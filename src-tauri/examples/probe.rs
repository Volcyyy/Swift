//! Live diagnostic for the Bungie API path that feeds the timer.
//!
//! Deliberately standalone: it shares no code with the app, so what it reports
//! is what the API actually did, not what our parsing thinks it did.
//!
//! Lives in `examples/` rather than `src/bin/` on purpose: `cargo build` skips
//! examples, so `tauri dev` never tries to relink this while it is running --
//! which Windows refuses, failing the app's build.
//!
//! Usage (PowerShell):
//!     $env:BUNGIE_API_KEY = "<your key>"
//!     cargo run --example probe -- clock
//!     cargo run --example probe -- watch
//!
//! `clock`  measures how far your PC clock is from Bungie's. Any drift here is
//!          subtracted straight off the timer -- it is the "starts late and
//!          finishes late by a constant amount" cause.
//! `watch`  polls the current activity once a second and, every time you load
//!          into or out of an activity, reports exactly how late the app could
//!          possibly have noticed, split into Bungie's lag and our own.

use std::{
    collections::HashMap,
    env, fs,
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, DATE, USER_AGENT},
    Client,
};
use serde_json::Value;

const API_PATH: &str = "https://www.bungie.net/Platform";
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const CLOCK_SAMPLES: usize = 20;
const MINT_SAMPLES: usize = 8;

// Mirrors the app: only skew provably beyond this is corrected. A window can
// exclude zero and still have a midpoint further from the truth than zero was.
const MIN_CORRECTABLE_SKEW_MILLIS: i64 = 1_000;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("\nerror: {e:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();

    // Broken or half-working IPv6 is a classic cause of connections that hang
    // rather than fail. Forcing IPv4 is the quickest way to rule it in or out.
    let ipv4_only = args.iter().any(|a| a == "ipv4");
    let mode = args
        .iter()
        .find(|a| *a != "ipv4")
        .cloned()
        .unwrap_or_else(|| "watch".to_string());

    if !matches!(mode.as_str(), "clock" | "watch" | "mint") {
        bail!("unknown mode {mode:?}; expected `clock`, `watch` or `mint`, optionally with `ipv4`");
    }

    let api_key = env::var("BUNGIE_API_KEY")
        .context("BUNGIE_API_KEY is not set in this shell")?;

    if ipv4_only {
        println!("Forcing IPv4.\n");
    }

    let probe = Probe::new(&api_key, ipv4_only)?;

    match mode.as_str() {
        "clock" => probe.clock().await,
        "mint" => probe.mint().await,
        _ => probe.watch().await,
    }
}

struct Probe {
    client: Client,
    membership_type: usize,
    membership_id: String,
}

/// One request, with the timings needed to reason about lag.
struct Timed {
    body: Value,
    sent_at: DateTime<Utc>,
    received_at: DateTime<Utc>,
    server_date: Option<DateTime<Utc>>,
    /// `Age` header: how long Cloudflare had been holding this response.
    cache_age_seconds: Option<u64>,
    /// `cf-cache-status`: HIT, MISS, BYPASS, STALE...
    cache_status: Option<String>,
    /// `responseMintedTimestamp`: when Bungie generated the payload. The one
    /// number that separates "their data pipeline is slow" from "we are being
    /// handed an old snapshot" -- the CDN can call a response fresh while what
    /// is inside it was minted minutes ago.
    minted_at: Option<DateTime<Utc>>,
}

impl Timed {
    fn rtt_millis(&self) -> i64 {
        (self.received_at - self.sent_at).num_milliseconds()
    }

    /// Local time at the moment the server handled the request: the midpoint of
    /// the round trip.
    fn handled_at(&self) -> DateTime<Utc> {
        self.sent_at + chrono::Duration::milliseconds(self.rtt_millis() / 2)
    }

    /// Bounds on (server - local) in milliseconds. True server time is in
    /// [header, header + 1s), stamped at some local instant between sending and
    /// receiving; pairing the extremes bounds the difference. Mirrors the
    /// app's `offset_bounds`.
    fn offset_bounds(&self) -> Option<(i64, i64)> {
        let header = self.server_date?;

        Some((
            (header - self.received_at).num_milliseconds(),
            (header + chrono::Duration::seconds(1) - self.sent_at).num_milliseconds(),
        ))
    }
}

impl Probe {
    fn new(api_key: &str, ipv4_only: bool) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("Swift-probe/1"));
        headers.insert(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(api_key).context("API key is not a valid header value")?,
        );

        // Matches the app's client so the timings are representative.
        let mut builder = Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(4))
            .timeout(Duration::from_secs(8))
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(4)
            .tcp_keepalive(Duration::from_secs(30));

        if ipv4_only {
            // Binding the source to an IPv4 address forces the whole connection
            // onto IPv4.
            builder = builder.local_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        }

        let client = builder.build()?;

        let (membership_type, membership_id) = selected_profile()?;

        Ok(Self {
            client,
            membership_type,
            membership_id,
        })
    }

    async fn get(&self, path: &str) -> Result<Timed> {
        let sent_at = Utc::now();
        let resp = self.client.get(format!("{API_PATH}{path}")).send().await?;
        let received_at = Utc::now();

        let server_date = resp
            .headers()
            .get(DATE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| DateTime::parse_from_rfc2822(s).ok())
            .map(|d| d.with_timezone(&Utc));

        let cache_age_seconds = resp
            .headers()
            .get("age")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok());

        let cache_status = resp
            .headers()
            .get("cf-cache-status")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let status = resp.status();
        let text = resp.text().await?;
        let body: Value = serde_json::from_str(&text)
            .with_context(|| format!("HTTP {status} body was not JSON: {text:.200}"))?;

        let code = body.get("ErrorCode").and_then(Value::as_i64).unwrap_or(-1);

        if code != 1 {
            let message = body
                .get("Message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            bail!("Bungie error {code}: {message}");
        }

        let response = body
            .get("Response")
            .cloned()
            .ok_or_else(|| anyhow!("response object missing"))?;

        let minted_at = response
            .get("responseMintedTimestamp")
            .and_then(Value::as_str)
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&Utc))
            // Bungie writes an unset timestamp as year 1 rather than omitting it.
            .filter(|d| d.timestamp() > 0);

        Ok(Timed {
            body: response,
            sent_at,
            received_at,
            server_date,
            cache_age_seconds,
            cache_status,
            minted_at,
        })
    }

    // 204 is what the app polls. 1000 (profileTransitoryData) is Bungie's
    // live fireteam/joinability component, minted on its own cadence -- hence
    // the separate `secondaryComponentsMintedTimestamp` -- so it is the one
    // plausible source of a fresher activity start time.
    //
    // The trailing slash before the query is load-bearing: without it Bungie
    // answers 307 and every call costs two round trips instead of one.
    fn current_activity_path(&self) -> String {
        format!(
            "/Destiny2/{}/Profile/{}/?components=204,1000",
            self.membership_type, self.membership_id
        )
    }

    // ---- clock -----------------------------------------------------------

    async fn clock(&self) -> Result<()> {
        println!("Measuring your clock against Bungie's, {CLOCK_SAMPLES} samples\n");

        // Each response only says the offset lies somewhere in a window a bit
        // over a second wide, because the Date header names a whole second and
        // hides where inside it the stamp fell. Two samples that land on the
        // same phase of the second say nearly the same thing, so the gaps below
        // are deliberately not a whole number of seconds: they walk the phase
        // around, and the windows then cut each other down.
        println!("        rtt      handled       header    offset window");

        let mut bounds: Vec<(i64, i64)> = Vec::new();
        let mut rtts: Vec<i64> = Vec::new();
        let mut failures = 0;

        for i in 1..=CLOCK_SAMPLES {
            let timed = match self.get(self.current_activity_path().as_str()).await {
                Ok(t) => t,
                Err(e) => {
                    // A stalled request is worth seeing, not worth aborting for.
                    println!("  {i:>2}.  request failed: {e:#}");
                    failures += 1;
                    continue;
                }
            };

            match (timed.offset_bounds(), timed.server_date) {
                (Some((lo, hi)), Some(header)) => {
                    let rtt = timed.rtt_millis();
                    // The first request pays for TCP + TLS setup; the rest reuse
                    // the pooled connection. The gap is what the shared client
                    // saves on every poll.
                    let note = if i == 1 { "  cold" } else { "" };
                    println!(
                        "  {i:>2}. {rtt:>5}ms  {}  {}  [{lo:>+6}, {hi:>+6}]ms{note}",
                        stamp(timed.handled_at()),
                        stamp(header)
                    );
                    bounds.push((lo, hi));
                    rtts.push(rtt);
                }
                _ => println!("  {i:>2}.  no usable Date header"),
            }

            // Walks the phase of the second so the windows actually differ.
            tokio::time::sleep(Duration::from_millis(200 + (i as u64 * 137) % 700)).await;
        }

        if bounds.is_empty() {
            bail!("no usable samples ({failures} requests failed)");
        }

        let (lo, hi) = bounds
            .iter()
            .fold((i64::MIN, i64::MAX), |(l, h), (bl, bh)| (l.max(*bl), h.min(*bh)));

        // Contradictory windows mean the clock moved under us mid-run.
        if lo > hi {
            println!("\n  INCONCLUSIVE: the samples contradict each other.");
            println!("  Your clock moved while measuring -- almost certainly Windows");
            println!("  slewing it. Re-run in a minute, once it has settled.");
            return Ok(());
        }

        let midpoint = (lo + hi) / 2;

        println!();

        if failures > 0 {
            let total = bounds.len() + failures;
            println!("  ** {failures} of {total} requests to Bungie timed out. **");
            println!("  Each one stalls for the full timeout, and the app is blind");
            println!("  to activity changes for that whole time. At this rate that");
            println!("  matters far more to your timer than any clock offset.");
            println!();
        }

        println!("  combined    : offset is between {lo:+}ms and {hi:+}ms");
        println!("  from        : {} good samples", bounds.len());

        if rtts.len() > 1 {
            let warm: Vec<i64> = rtts[1..].to_vec();
            let avg_warm = warm.iter().sum::<i64>() / warm.len() as i64;
            println!("  connection  : {}ms cold vs {avg_warm}ms warm", rtts[0]);
            println!("                every poll paid the cold price before connection reuse");
        }

        println!();

        println!("  window width: {}ms over {} samples", hi - lo, bounds.len());
        println!();

        let correctable = lo > MIN_CORRECTABLE_SKEW_MILLIS || hi < -MIN_CORRECTABLE_SKEW_MILLIS;

        if correctable {
            let direction = if midpoint > 0 { "behind" } else { "ahead of" };
            println!("  Your PC clock is {direction} Bungie's by between {:.2}s and {:.2}s.",
                lo.abs().min(hi.abs()) as f64 / 1000.0,
                lo.abs().max(hi.abs()) as f64 / 1000.0);
            println!();
            println!("  The timer read about {:.2}s {} for the whole activity --",
                (midpoint.abs() as f64) / 1000.0,
                if midpoint > 0 { "low" } else { "high" });
            println!("  starting late and finishing late by that much. The app");
            println!("  corrects by {midpoint:+}ms.");
        } else if lo <= 0 && 0 <= hi {
            println!("  Your clock cannot be told apart from Bungie's: zero is still");
            println!("  inside the window. It is off by at most {:.2}s either way,",
                hi.max(-lo) as f64 / 1000.0);
            println!("  which is far too little to explain a late timer.");
            println!("  The app applies no correction.");
        } else {
            // Zero is excluded, but the window still reaches under the bar, so
            // the midpoint could easily sit further from the truth than zero.
            println!("  Your clock is off by between {:.2}s and {:.2}s, but that is",
                lo.abs().min(hi.abs()) as f64 / 1000.0,
                lo.abs().max(hi.abs()) as f64 / 1000.0);
            println!("  under the {:.0}ms the app requires before touching the timer.",
                MIN_CORRECTABLE_SKEW_MILLIS as f64);
            println!("  Correcting on a window this wide can add more error than it");
            println!("  removes, and sub-second drift is dwarfed by Bungie's own lag.");
            println!("  The app applies no correction.");
        }

        println!();
        println!("  For an exact answer, Windows has a real NTP client built in:");
        println!("      w32tm /stripchart /computer:time.windows.com /samples:5 /dataonly");

        Ok(())
    }

    // ---- mint ------------------------------------------------------------

    // The payload arrives already minutes old. This asks the one question that
    // decides whether that is recoverable: does a request Cloudflare cannot
    // serve from cache come back with a fresher `responseMintedTimestamp`?
    async fn mint(&self) -> Result<()> {
        println!("Comparing payload freshness, plain vs cache-bypassed.
");
        println!("  round        plain                        bypassed");

        let mut plain_ages = Vec::new();
        let mut bypass_ages = Vec::new();

        for i in 1..=MINT_SAMPLES {
            let plain = self.get(&self.current_activity_path()).await;

            // A query parameter Bungie ignores but Cloudflare keys on, making
            // this a cache miss every time.
            let unique = Utc::now().timestamp_millis();
            let busted = self
                .get(&format!("{}&_cb={unique}", self.current_activity_path()))
                .await;

            let describe = |r: &Result<Timed>| match r {
                Err(e) => format!("{e:#}"),
                Ok(t) => match t.minted_at {
                    None => "no mint timestamp".to_string(),
                    Some(m) => format!(
                        "{:>6} old  [{}]",
                        secs(t.received_at - m),
                        t.cache_status.clone().unwrap_or_else(|| "-".to_string())
                    ),
                },
            };

            println!("  {i:>2}.   {:<28} {}", describe(&plain), describe(&busted));

            if let Ok(t) = &plain {
                if let Some(m) = t.minted_at {
                    plain_ages.push((t.received_at - m).num_milliseconds());
                }
            }
            if let Ok(t) = &busted {
                if let Some(m) = t.minted_at {
                    bypass_ages.push((t.received_at - m).num_milliseconds());
                }
            }

            tokio::time::sleep(Duration::from_secs(3)).await;
        }

        let mean = |v: &Vec<i64>| {
            if v.is_empty() {
                None
            } else {
                Some(v.iter().sum::<i64>() / v.len() as i64)
            }
        };

        println!();

        match (mean(&plain_ages), mean(&bypass_ages)) {
            (Some(p), Some(b)) => {
                println!("  plain    payload age: {:.1}s average", p as f64 / 1000.0);
                println!("  bypassed payload age: {:.1}s average", b as f64 / 1000.0);
                println!();

                let saved = p - b;

                if saved > 10_000 {
                    println!("  Bypassing the cache gets a payload {:.0}s fresher.", saved as f64 / 1000.0);
                    println!("  That much of the timer's delay is recoverable.");
                } else {
                    println!("  No meaningful difference. The payload is already this old");
                    println!("  when Bungie mints it, so the delay is inside their pipeline");
                    println!("  and bypassing the cache would cost them load for nothing.");
                }
            }
            _ => println!("  Not enough usable samples."),
        }

        Ok(())
    }

    // ---- watch -----------------------------------------------------------

    async fn watch(&self) -> Result<()> {
        println!("Polling current activity every {}s. Load into an activity to", POLL_INTERVAL.as_secs());
        println!("see how late it could be detected. Ctrl+C to stop.\n");

        let mut names: HashMap<u64, String> = HashMap::new();
        let mut bounds: Option<(i64, i64)> = None; // offset window
        let mut state: Option<(DateTime<Utc>, u64)> = None;
        let mut stale_seen: Option<(DateTime<Utc>, u64)> = None;
        let mut transitory_seen: Option<DateTime<Utc>> = None;
        let mut announced_transitory = false;
        let mut previous: Option<Timed> = None;

        loop {
            let timed = match self.get(self.current_activity_path().as_str()).await {
                Ok(t) => t,
                Err(e) => {
                    println!("  poll failed: {e:#}");
                    tokio::time::sleep(POLL_INTERVAL).await;
                    continue;
                }
            };

            if let Some((lo, hi)) = timed.offset_bounds() {
                bounds = match bounds {
                    Some((plo, phi)) if plo.max(lo) <= phi.min(hi) => {
                        Some((plo.max(lo), phi.min(hi)))
                    }
                    // Contradiction means the clock stepped; start over.
                    _ => Some((lo, hi)),
                };
            }

            // Only correct when the bounds rule out synced clocks, matching
            // what the app itself will do.
            let offset_millis = best_offset_millis(&bounds);

            // Report once what the live component is actually giving us, so a
            // silent "no data" is not mistaken for "never any fresher".
            if !announced_transitory {
                println!("  transitory component: {}", transitory_availability(&timed.body));
                announced_transitory = true;
            }

            // Raced against component 204 below. If this consistently reports an
            // activity earlier, the app should be polling it instead.
            if let Some(start) = transitory_start(&timed.body) {
                if transitory_seen != Some(start) {
                    if transitory_seen.is_some() {
                        let seen_at =
                            correct_by(timed.received_at, best_offset_millis(&bounds));

                        println!(
                            "  LIVE   transitory reports an activity started {} -> {} late",
                            stamp(start),
                            secs(seen_at - start)
                        );
                    }

                    transitory_seen = Some(start);
                }
            }

            let latest = latest_character_activity(&timed.body)?;

            // Bungie's backends do not agree with each other, so a response can
            // carry an activity older than one already seen. The app discards
            // these (update_current returns early on an older start date), so
            // report them as what they are instead of a phantom entry.
            if let Some((newest, _)) = state {
                if latest.0 < newest {
                    if stale_seen != Some(latest) {
                        println!(
                            "  STALE  served {} (from {}), {} behind -- ignored",
                            self.name_of(latest.1, &mut names).await,
                            stamp(latest.0),
                            secs(newest - latest.0)
                        );
                        stale_seen = Some(latest);
                    }

                    previous = Some(timed);
                    tokio::time::sleep(POLL_INTERVAL).await;
                    continue;
                }
            }

            if state.as_ref() != Some(&latest) {
                let first_seen = state.is_none();
                state = Some(latest);

                let (started, hash) = latest;

                if first_seen {
                    // Nothing to measure on the first poll; just show the baseline.
                    println!(
                        "  baseline: {} (hash {hash}), started {}",
                        self.name_of(hash, &mut names).await,
                        stamp(started)
                    );
                } else {
                    let label = if hash == 0 { "LEFT " } else { "ENTER" };
                    let name = self.name_of(hash, &mut names).await;

                    println!("\n  {label}  {name}  (hash {hash})");

                    self.report_lateness(started, &timed, previous.as_ref(), offset_millis);
                }
            }

            previous = Some(timed);
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Everything here is put on Bungie's timescale by applying the measured
    /// clock offset, so the numbers are lag and not clock drift.
    fn report_lateness(
        &self,
        started: DateTime<Utc>,
        current: &Timed,
        previous: Option<&Timed>,
        offset_millis: i64,
    ) {
        let correct = |t: DateTime<Utc>| t + chrono::Duration::milliseconds(offset_millis);

        let seen_at = correct(current.received_at);
        let served_at = correct(current.handled_at());

        println!("    started (Bungie clock) : {}", stamp(started));
        println!(
            "    first seen             : {}   ->  {} late",
            stamp(seen_at),
            secs(seen_at - started)
        );

        // The API was already serving it when this poll was handled, and was
        // not when the previous one was, so the changeover falls between them.
        match previous {
            Some(prev) => {
                let not_yet = correct(prev.handled_at());
                println!(
                    "    Bungie began serving   : between {} and {} after start",
                    secs(not_yet - started),
                    secs(served_at - started)
                );
            }
            None => println!(
                "    Bungie was serving by  : {} after start",
                secs(served_at - started)
            ),
        }

        println!(
            "    added by this client   : {}   (network + poll cadence)",
            secs(seen_at - served_at)
        );
        println!("    clock correction used  : {offset_millis:+}ms");

        if let Some(age) = current.cache_age_seconds {
            println!("    served from CDN cache  : {age}s old");
        }

        if let Some(minted) = current.minted_at {
            let payload_age = correct(current.received_at) - minted;

            println!(
                "    Bungie minted payload  : {} ({} before we saw it)",
                stamp(minted),
                secs(payload_age)
            );

            // If the payload was minted just now yet already describes an
            // activity two minutes underway, the delay is inside Bungie's own
            // pipeline and no client can shorten it. If the payload itself is
            // old, we are being served a stale snapshot, which might not be.
            let verdict = if payload_age.num_seconds() * 2 > (seen_at - started).num_seconds() {
                "stale snapshot: most of the delay is the payload's own age"
            } else {
                "fresh payload: the delay is inside Bungie's pipeline"
            };

            println!("    -> {verdict}");
        }
    }

    async fn name_of(&self, hash: u64, cache: &mut HashMap<u64, String>) -> String {
        if hash == 0 {
            return "orbit / no activity".to_string();
        }

        if let Some(name) = cache.get(&hash) {
            return name.clone();
        }

        let name = match self
            .get(&format!("/Destiny2/Manifest/DestinyActivityDefinition/{hash}/"))
            .await
        {
            Ok(t) => t
                .body
                .pointer("/originalDisplayProperties/name")
                .or_else(|| t.body.pointer("/displayProperties/name"))
                .and_then(Value::as_str)
                .unwrap_or("unnamed activity")
                .to_string(),
            Err(_) => "unknown activity".to_string(),
        };

        cache.insert(hash, name.clone());
        name
    }
}

fn correct_by(t: DateTime<Utc>, offset_millis: i64) -> DateTime<Utc> {
    t + chrono::Duration::milliseconds(offset_millis)
}

fn best_offset_millis(bounds: &Option<(i64, i64)>) -> i64 {
    match bounds {
        Some((lo, hi)) if *lo > MIN_CORRECTABLE_SKEW_MILLIS || *hi < -MIN_CORRECTABLE_SKEW_MILLIS => {
            (lo + hi) / 2
        }
        _ => 0,
    }
}

/// Activity start time as reported by the transitory component, if the account
/// exposes it. Carries no activity hash -- only when the activity began.
fn transitory_start(response: &Value) -> Option<DateTime<Utc>> {
    response
        .pointer("/profileTransitoryData/data/currentActivity/startTime")
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc))
}

/// Whether the transitory component came back at all, and under what privacy.
fn transitory_availability(response: &Value) -> String {
    match response.get("profileTransitoryData") {
        None => "not returned (component not served for this account)".to_string(),
        Some(t) => {
            let privacy = t.get("privacy").and_then(Value::as_u64).unwrap_or(0);
            let has_data = t.get("data").map_or(false, |d| !d.is_null());

            if has_data {
                format!("available (privacy {privacy})")
            } else {
                format!("returned but empty (privacy {privacy}; 2 = private)")
            }
        }
    }
}

/// The most recently started activity across all characters, matching how the
/// app picks one.
fn latest_character_activity(response: &Value) -> Result<(DateTime<Utc>, u64)> {
    let data = response
        .pointer("/characterActivities/data")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            anyhow!("no characterActivities data -- is the account's privacy set to public?")
        })?;

    data.values()
        .filter_map(|c| {
            let started = c
                .get("dateActivityStarted")
                .and_then(Value::as_str)
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())?
                .with_timezone(&Utc);

            let hash = c
                .get("currentActivityHash")
                .and_then(Value::as_u64)
                .unwrap_or(0);

            Some((started, hash))
        })
        .max_by_key(|(started, _)| *started)
        .ok_or_else(|| anyhow!("no character data for profile"))
}

fn selected_profile() -> Result<(usize, String)> {
    let path = profiles_path()?;

    let text = fs::read_to_string(&path).with_context(|| {
        format!(
            "could not read {}; open Swift and select a profile first",
            path.display()
        )
    })?;

    let json: Value = serde_json::from_str(&text)?;

    let selected = json
        .get("selectedProfile")
        .filter(|v| !v.is_null())
        .ok_or_else(|| anyhow!("no profile selected in {}", path.display()))?;

    let membership_type = selected
        .get("accountPlatform")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("profile has no accountPlatform"))? as usize;

    let membership_id = selected
        .get("accountId")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("profile has no accountId"))?
        .to_string();

    println!("Profile {membership_id} (platform {membership_type})\n");

    Ok((membership_type, membership_id))
}

fn profiles_path() -> Result<PathBuf> {
    let mut path = PathBuf::from(
        env::var("APPDATA").context("APPDATA is not set; this probe targets Windows")?,
    );
    path.push("Swift");
    path.push("profiles.json");
    Ok(path)
}

fn stamp(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
        .split('T')
        .nth(1)
        .unwrap_or("?")
        .trim_end_matches('Z')
        .to_string()
}

fn secs(d: chrono::Duration) -> String {
    format!("{:.2}s", d.num_milliseconds() as f64 / 1000.0)
}
