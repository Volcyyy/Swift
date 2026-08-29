use std::{
    error::Error,
    fmt::{Display, Formatter},
    sync::{Mutex, OnceLock},
    time::Duration,
};

use chrono::{DateTime, Utc};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, DATE, USER_AGENT as USER_AGENT_HEADER},
    Client, Method, RequestBuilder,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::consts::{api_key, API_CONNECT_TIMEOUT, API_PATH, API_REQUEST_TIMEOUT, USER_AGENT};

pub enum BungieRequest<'a> {
    SearchDestinyPlayerByBungieName {
        display_name: &'a str,
        display_name_code: usize,
    },
    GetProfile {
        membership_type: usize,
        membership_id: &'a str,
        component: usize,
        /// Adds a throwaway query parameter so Cloudflare cannot serve this
        /// from cache. See `path()` for why that is necessary.
        cache_bust: bool,
    },
    GetActivityHistory {
        membership_type: usize,
        membership_id: &'a str,
        character_id: &'a str,
        page: usize,
    },
    GetDestinyActivityDefinition {
        activity_hash: usize,
    },
}

impl BungieRequest<'_> {
    /// Path and query for this request.
    ///
    /// Every route ends in a trailing slash. Bungie answers 307 and redirects
    /// to the slashed form otherwise, which costs a second round trip on every
    /// single call -- doubling both latency and the exposure to a stalled
    /// connection. `every_route_keeps_its_trailing_slash` guards this.
    fn path(&self) -> String {
        match self {
            BungieRequest::SearchDestinyPlayerByBungieName { .. } => {
                "/Destiny2/SearchDestinyPlayerByBungieName/All/".to_string()
            }
            BungieRequest::GetProfile {
                membership_type,
                membership_id,
                component,
                cache_bust,
            } => {
                let mut path = format!(
                    "/Destiny2/{membership_type}/Profile/{membership_id}/?components={component}"
                );

                // Bungie fronts this endpoint with Cloudflare, and measured
                // against the live API the cache serves payloads 48-188s old
                // (101s mean) while the origin has the data within ~1.3s. For
                // an endpoint whose whole purpose is "what is happening right
                // now", a cache hit is worse than useless.
                //
                // Cloudflare ignores request-side Cache-Control, so the only
                // lever is the cache key: a parameter Bungie ignores but
                // Cloudflare keys on. Reserved for the current-activity poll --
                // everything else is happy with cached data.
                if *cache_bust {
                    path.push_str(&format!("&_={}", Utc::now().timestamp_millis()));
                }

                path
            }
            BungieRequest::GetActivityHistory {
                membership_type,
                membership_id,
                character_id,
                page,
            } => format!(
                "/Destiny2/{membership_type}/Account/{membership_id}/Character/{character_id}/Stats/Activities/?mode=7&count=25&page={page}"
            ),
            BungieRequest::GetDestinyActivityDefinition { activity_hash } => {
                format!("/Destiny2/Manifest/DestinyActivityDefinition/{activity_hash}/")
            }
        }
    }

    fn method(&self) -> Method {
        match self {
            BungieRequest::SearchDestinyPlayerByBungieName { .. } => Method::POST,
            _ => Method::GET,
        }
    }

    fn body(&self) -> Option<String> {
        match self {
            BungieRequest::SearchDestinyPlayerByBungieName {
                display_name,
                display_name_code,
            } => Some(
                json!({"displayName": display_name, "displayNameCode": display_name_code})
                    .to_string(),
            ),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct BungieResponseStatus {
    error_code: isize,
    message: String,
    throttle_seconds: isize,
    response: Option<Value>,
}

#[derive(Debug)]
pub enum BungieResponseError {
    DeserializeError {
        err: serde_json::Error,
        status_code: u16,
    },
    BungieError {
        message: String,
        error_code: isize,
        throttle_seconds: isize,
    },
    ResponseMissing,
    NetworkError(anyhow::Error),
}

impl Display for BungieResponseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            BungieResponseError::DeserializeError { err, status_code } => {
                write!(f, "Failed to parse response (code {status_code}): {err}")
            }
            BungieResponseError::BungieError {
                message,
                error_code,
                throttle_seconds,
            } => {
                if *throttle_seconds > 0 {
                    write!(
                        f,
                        "{message} ({error_code}), throttled! ({throttle_seconds}s)"
                    )
                } else {
                    write!(f, "{message} ({error_code})")
                }
            }
            BungieResponseError::ResponseMissing => f.write_str("Response object missing"),
            BungieResponseError::NetworkError(e) => e.fmt(f),
        }
    }
}

impl Error for BungieResponseError {}

// A single shared client, so polls reuse a warm connection instead of paying
// for a fresh TCP + TLS handshake on every request.
fn client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();

    CLIENT.get_or_init(|| {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT_HEADER, HeaderValue::from_static(USER_AGENT));
        let key = api_key();

        if key.is_empty() {
            // Every request will come back 2101 otherwise, which reads like a
            // bad key rather than a missing one.
            eprintln!(
 "No Bungie API key. Set BUNGIE_API_KEY in the environment, or build with it set to compile one in."
            );
        }

        headers.insert(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_str(key).expect("Bungie API key is not a valid header value"),
        );

        Client::builder()
            .default_headers(headers)
            .connect_timeout(API_CONNECT_TIMEOUT)
            .timeout(API_REQUEST_TIMEOUT)
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(4)
            .tcp_keepalive(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client")
    })
}

// Activity start times come from Bungie's clock, but the timer counts up
// against the local one. Windows only re-syncs its clock weekly by default, so
// a few seconds of drift is normal, and every one of those seconds shows up as
// the timer starting and finishing late by exactly that much.
//
// Each response bounds the offset rather than pinning it: the Date header names
// a whole second, and it was stamped at some unknown point between us sending
// the request and receiving it. One response therefore only narrows the offset
// to a window a bit over a second wide. Intersecting the windows from responses
// that land on different phases of the second shrinks it much further -- and it
// yields real bounds, so the app can tell "definitely off by a second" apart
// from "too small to see".
#[derive(Clone, Copy)]
struct ClockBounds {
    /// Tightest known bounds on (server - local), in milliseconds.
    lo: i64,
    hi: i64,
    started_at: DateTime<Utc>,
    samples: usize,
}

impl ClockBounds {
    fn midpoint_millis(&self) -> i64 {
        (self.lo + self.hi) / 2
    }

    /// True when the bounds put the offset entirely beyond `threshold` in one
    /// direction, so a correction is certain to be worth more than it risks.
    fn definitely_exceeds(&self, threshold: i64) -> bool {
        self.lo > threshold || self.hi < -threshold
    }
}

fn clock_bounds() -> &'static Mutex<Option<ClockBounds>> {
    static CLOCK_BOUNDS: OnceLock<Mutex<Option<ClockBounds>>> = OnceLock::new();
    CLOCK_BOUNDS.get_or_init(|| Mutex::new(None))
}

// Samples slower than this say more about the network than about the clock.
const MAX_USABLE_RTT_MILLIS: i64 = 5000;

// One sample can be a fluke or a cached header; two agreeing is enough to act.
const MIN_SAMPLES_TO_CORRECT: usize = 2;

// Only skew provably larger than this is corrected.
//
// Excluding zero is not enough on its own. A window like [+77ms, +586ms] rules
// out a synced clock, but its midpoint sits 250ms from a true offset that was
// really 82ms -- so "correcting" would have added more error than it removed.
// Sub-second drift is in any case dwarfed by Bungie's own data lag, so there is
// nothing to win by chasing it. Multi-second drift, the kind that visibly moves
// a timer, clears this bar with room to spare.
const MIN_CORRECTABLE_SKEW_MILLIS: i64 = 1_000;

// Bounds are rebuilt from scratch past this age so the estimate keeps following
// the local clock as it drifts, rather than pinning it to where it once was.
const CLOCK_BOUNDS_MAX_AGE_SECONDS: i64 = 300;

/// Bounds `(lo, hi)` on `server - local` in milliseconds implied by one
/// response, or None if the round trip was too slow or nonsensical to bound
/// anything usefully.
fn offset_bounds(
    server_date: DateTime<Utc>,
    sent_at: DateTime<Utc>,
    received_at: DateTime<Utc>,
) -> Option<(i64, i64)> {
    let rtt_millis = (received_at - sent_at).num_milliseconds();

    if rtt_millis < 0 || rtt_millis > MAX_USABLE_RTT_MILLIS {
        return None;
    }

    // True server time is in [header, header + 1s), stamped at a local instant
    // in [sent_at, received_at]. Pairing the extremes bounds the difference.
    let lo = (server_date - received_at).num_milliseconds();
    let hi = (server_date + chrono::Duration::seconds(1) - sent_at).num_milliseconds();

    Some((lo, hi))
}

fn record_clock_sample(
    date_header: Option<&HeaderValue>,
    sent_at: DateTime<Utc>,
    received_at: DateTime<Utc>,
) {
    let server_date = match date_header
        .and_then(|v| v.to_str().ok())
        .and_then(|s| DateTime::parse_from_rfc2822(s).ok())
    {
        Some(d) => d.with_timezone(&Utc),
        None => return,
    };

    let (lo, hi) = match offset_bounds(server_date, sent_at, received_at) {
        Some(b) => b,
        None => return,
    };

    let fresh = ClockBounds {
        lo,
        hi,
        started_at: received_at,
        samples: 1,
    };

    let mut lock = match clock_bounds().lock() {
        Ok(l) => l,
        Err(_) => return,
    };

    *lock = Some(match *lock {
        Some(prev)
            if (received_at - prev.started_at).num_seconds() < CLOCK_BOUNDS_MAX_AGE_SECONDS =>
        {
            let merged = ClockBounds {
                lo: prev.lo.max(lo),
                hi: prev.hi.min(hi),
                samples: prev.samples + 1,
                ..prev
            };

            // Bounds that contradict each other mean the clock stepped under
            // us. Nothing learned before that is worth keeping.
            if merged.lo <= merged.hi {
                merged
            } else {
                fresh
            }
        }
        _ => fresh,
    });
}

/// Milliseconds to add to the local clock to line it up with Bungie's, or 0
/// when the two cannot be told apart.
///
/// Correcting by an amount the measurement cannot resolve would trade a known
/// sub-second error for an unknown one, so this stays silent unless the bounds
/// prove the skew is larger than `MIN_CORRECTABLE_SKEW_MILLIS`.
pub fn clock_offset_millis() -> i64 {
    clock_bounds()
        .lock()
        .ok()
        .and_then(|l| *l)
        .filter(|b| {
            b.samples >= MIN_SAMPLES_TO_CORRECT
                && b.definitely_exceeds(MIN_CORRECTABLE_SKEW_MILLIS)
        })
        .map(|b| b.midpoint_millis())
        .unwrap_or(0)
}

fn api_request(path: &str, method: Method) -> RequestBuilder {
    client().request(method, format!("{API_PATH}{path}"))
}

pub async fn make_request(req: BungieRequest<'_>) -> Result<Value, BungieResponseError> {
    make_request_with_timeout(req, None).await
}

/// `timeout` overrides the client-wide deadline for this one request. Used by
/// polls whose answer goes stale faster than the default allows.
pub async fn make_request_with_timeout(
    req: BungieRequest<'_>,
    timeout: Option<std::time::Duration>,
) -> Result<Value, BungieResponseError> {
    let mut builder = api_request(&req.path(), req.method());

    if let Some(body) = req.body() {
        builder = builder.body(body);
    }

    let builder = match timeout {
        Some(t) => builder.timeout(t),
        None => builder,
    };

    let sent_at = Utc::now();

    let resp = builder
        .send()
        .await
        .map_err(|e| BungieResponseError::NetworkError(e.into()))?;

    // Taken as soon as the headers land, before the body is read, to keep the
    // round-trip measurement as tight as possible.
    record_clock_sample(resp.headers().get(DATE), sent_at, Utc::now());

    let status_code = resp.status().as_u16();

    let text = resp
        .text()
        .await
        .map_err(|e| BungieResponseError::NetworkError(e.into()))?;

    let status: BungieResponseStatus = match serde_json::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            return Err(BungieResponseError::DeserializeError {
                err: e,
                status_code,
            }
            .into())
        }
    };

    if status.error_code != 1 {
        return Err(BungieResponseError::BungieError {
            message: status.message,
            error_code: status.error_code,
            throttle_seconds: status.throttle_seconds,
        }
        .into());
    }

    Ok(status
        .response
        .ok_or(BungieResponseError::ResponseMissing)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().with_timezone(&Utc)
    }

    fn ms(n: i64) -> chrono::Duration {
        chrono::Duration::milliseconds(n)
    }

    /// Bounds from one request against a server whose clock leads the local one
    /// by `true_offset_millis`.
    fn sample(true_offset_millis: i64, rtt_millis: i64, sent_at_millis: i64) -> (i64, i64) {
        let sent_at = at("2024-01-01T12:00:00Z") + ms(sent_at_millis);
        let received_at = sent_at + ms(rtt_millis);

        // The server handles the request halfway through the round trip and
        // stamps its own clock, truncated to a whole second.
        let server_now = sent_at + ms(rtt_millis / 2) + ms(true_offset_millis);
        let header = server_now - ms(server_now.timestamp_subsec_millis() as i64);

        offset_bounds(header, sent_at, received_at).expect("sample should be usable")
    }

    fn intersect(bounds: &[(i64, i64)]) -> (i64, i64) {
        bounds.iter().fold((i64::MIN, i64::MAX), |(lo, hi), (l, h)| (lo.max(*l), hi.min(*h)))
    }

    #[test]
    fn bounds_always_contain_the_true_offset() {
        for true_offset in [-4_000, -1_000, 0, 250, 1_000, 3_000] {
            for sent_at_millis in (0..1000).step_by(50) {
                let (lo, hi) = sample(true_offset, 120, sent_at_millis);

                assert!(
                    lo <= true_offset && true_offset <= hi,
                    "offset {true_offset} escaped [{lo}, {hi}] at phase {sent_at_millis}ms"
                );
            }
        }
    }

    #[test]
    fn intersecting_phases_narrows_below_one_second() {
        // Samples landing on different phases of the second constrain each
        // other; one alone never can, because the header hides the phase.
        let bounds: Vec<(i64, i64)> = (0..1000)
            .step_by(100)
            .map(|phase| sample(1_500, 120, phase))
            .collect();

        let (lo, hi) = intersect(&bounds);

        assert!(lo <= 1_500 && 1_500 <= hi, "true offset lost: [{lo}, {hi}]");
        assert!(hi - lo < 400, "expected a tight window, got [{lo}, {hi}]");
    }

    #[test]
    fn a_synced_clock_never_looks_skewed() {
        let bounds: Vec<(i64, i64)> = (0..1000).step_by(50).map(|p| sample(0, 120, p)).collect();
        let (lo, hi) = intersect(&bounds);

        // Zero stays inside, so excludes_zero() stays false and no correction
        // is ever applied to a clock that is already right.
        assert!(lo <= 0 && 0 <= hi, "synced clock bounded away from zero: [{lo}, {hi}]");
    }

    #[test]
    fn replays_the_offsets_measured_against_bungie() {
        // Real output from `probe -- clock`, whose local clock was behind.
        // Each row is (rtt, local handled time, whole second in the Date header).
        let observed = [
            (512, "12:32:02.348", "12:32:03"),
            (504, "12:32:03.416", "12:32:04"),
            (672, "12:32:04.565", "12:32:05"),
        ];

        let bounds: Vec<(i64, i64)> = observed
            .iter()
            .map(|(rtt, handled, header)| {
                let handled = at(&format!("2024-01-01T{handled}Z"));
                let header = at(&format!("2024-01-01T{header}.000Z"));

                offset_bounds(header, handled - ms(rtt / 2), handled + ms(rtt / 2)).unwrap()
            })
            .collect();

        let (lo, hi) = intersect(&bounds);

        // The conclusion that matters: a synced clock is ruled out.
        assert!(lo > 0, "these samples rule out a synced clock, got [{lo}, {hi}]");

        // But they barely narrow, and that is the point. They were taken about
        // a second apart, so each landed on the same phase of the second and
        // told us almost nothing the previous one had not. Sweeping the phase
        // is what buys precision -- see intersecting_phases_narrows_below_one_second.
        assert!(hi - lo > 1_000, "expected a still-wide window, got [{lo}, {hi}]");
    }

    #[test]
    fn rejects_samples_that_teach_nothing() {
        let sent_at = at("2024-01-01T12:00:00Z");
        let header = at("2024-01-01T12:00:00Z");

        // Too slow to attribute the delay to either direction.
        assert!(offset_bounds(header, sent_at, sent_at + ms(MAX_USABLE_RTT_MILLIS + 1)).is_none());

        // Received before it was sent: the local clock stepped mid-request.
        assert!(offset_bounds(header, sent_at, sent_at - ms(1)).is_none());

        assert!(offset_bounds(header, sent_at, sent_at + ms(MAX_USABLE_RTT_MILLIS)).is_some());
    }

    #[test]
    fn only_corrects_skew_big_enough_to_be_worth_it() {
        let base = ClockBounds { lo: 0, hi: 0, started_at: at("2024-01-01T12:00:00Z"), samples: 4 };
        let threshold = MIN_CORRECTABLE_SKEW_MILLIS;

        // Straddling zero: indistinguishable from a synced clock.
        assert!(!ClockBounds { lo: -200, hi: 900, ..base }.definitely_exceeds(threshold));

        // Measured against Bungie in a real run: the window excluded zero, but
        // w32tm put the true offset at +82ms, near the low edge. Correcting by
        // the +331ms midpoint would have quadrupled the error, so this window
        // must not trigger one.
        let real_run = ClockBounds { lo: 77, hi: 586, ..base };
        assert!(!real_run.definitely_exceeds(threshold));
        assert!(real_run.lo <= 82 && 82 <= real_run.hi, "bounds must still hold the truth");

        // Multi-second drift, the kind that visibly moves a timer.
        let behind = ClockBounds { lo: 2_600, hi: 3_400, ..base };
        assert!(behind.definitely_exceeds(threshold));
        assert_eq!(behind.midpoint_millis(), 3_000);

        // Same in the other direction.
        assert!(ClockBounds { lo: -3_400, hi: -2_600, ..base }.definitely_exceeds(threshold));

        // Big but not provably so: part of the window is under the bar.
        assert!(!ClockBounds { lo: 900, hi: 3_000, ..base }.definitely_exceeds(threshold));
    }

    #[test]
    fn every_route_keeps_its_trailing_slash() {
        // Bungie answers 307 for any route without one, turning every call into
        // two round trips. Confirmed against the live API on all four routes.
        let requests = [
            BungieRequest::SearchDestinyPlayerByBungieName {
                display_name: "guardian",
                display_name_code: 1234,
            },
            BungieRequest::GetProfile {
                membership_type: 2,
                membership_id: "4611686018400000000",
                component: 204,
                cache_bust: true,
            },
            BungieRequest::GetActivityHistory {
                membership_type: 2,
                membership_id: "4611686018400000000",
                character_id: "2305843009300000000",
                page: 0,
            },
            BungieRequest::GetDestinyActivityDefinition {
                activity_hash: 313828469,
            },
        ];

        for req in requests {
            let path = req.path();
            let route = path.split('?').next().unwrap();

            assert!(
                route.ends_with('/'),
                "{path} would be redirected: the route must end in a slash"
            );
        }
    }

    #[test]
    fn the_current_activity_poll_defeats_the_cdn_cache() {
        let request = |cache_bust| BungieRequest::GetProfile {
            membership_type: 2,
            membership_id: "4611686018400000000",
            component: 204,
            cache_bust,
        };

        // Measured: cached responses carry payloads ~100s old, origin ~1.3s.
        // A repeated URL would be served from cache, so each poll must differ.
        let first = request(true).path();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = request(true).path();

        assert!(first.contains("&_="), "no cache-busting parameter in {first}");
        assert_ne!(first, second, "two polls shared a cache key");

        // Everything else should stay cacheable -- it is Bungie's bandwidth.
        assert!(!request(false).path().contains("&_="));
    }
}
