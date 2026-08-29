import "../core/global.css";
import "./overlay.css";
import { appWindow } from "@tauri-apps/api/window";
import { createPopup as _createPopup, type Popup } from "./popups";
import type { TauriEvent, Preferences, CurrentActivity, PlayerDataStatus } from "../core/types";
import { countClearsSince, destinyDailyReset, destinyWeeklyReset, determineActivityType, formatMillis, formatTime } from "../core/util";
import { getPlayerdata, getPreferences } from "../core/ipc";

const loaderElem = document.querySelector<HTMLElement>("#widget-loader")!;
const widgetContentElem = document.querySelector<HTMLElement>("#widget-content")!;
const timerElem = document.querySelector<HTMLElement>("#timer")!;
const timeElem = document.querySelector<HTMLElement>("#time")!;
const msElem = document.querySelector<HTMLElement>("#ms")!;
const dailyRow = document.querySelector<HTMLElement>("#daily-row")!;
const dailyElem = document.querySelector<HTMLElement>("#daily")!;
const weeklyElem = document.querySelector<HTMLElement>("#weekly")!;
const spotifyElem = document.querySelector<HTMLElement>("#spotify")!;
const spotifyTrack = document.querySelector<HTMLElement>("#spotify-track")!;
const spotifyArtist = document.querySelector<HTMLElement>("#spotify-artist")!;

let currentActivity: CurrentActivity;
let lastRaidId: any;
let doneInitialRefresh = false;
let shown = false;
let prefs: Preferences;
let timerTimeout: any;
let timerRunning = false;
let clockOffsetMillis = 0;
let spotifyInterval: any;

// After an activity ends the timer holds its last value instead of vanishing
// mid-count, then snaps to the duration Bungie reports for the run -- the same
// number the PGCR and any leaderboard shows. The live timer can only ever
// overshoot, because nothing knows the run is over until the API says so.
let frozenMillis: number | null = null;
let holdTimeout: any;
// The activity start the running timer is counting from, kept so freezing does
// not depend on currentActivity, which has already moved on by then.
let runningStartMillis = 0;
// Long enough for the history poll, which is nudged as soon as an activity
// ends, to bring back the finished run. If it never does, the held value goes
// away on its own rather than sitting there forever.
const HOLD_AWAITING_API_MS = 20000;
// How long the confirmed API time stays up once it arrives.
const HOLD_AFTER_API_MS = 8000;

async function init() {
  appWindow.listen("show", () => { if (!shown) { appWindow.show(); shown = true; checkTimerInterval(); } });
  appWindow.listen("hide", () => { if (shown) { appWindow.hide(); shown = false; checkTimerInterval(); } });
  applyPreferences(await getPreferences());
  refresh(await getPlayerdata());
  appWindow.listen("preferences_update", (p: TauriEvent<Preferences>) => applyPreferences(p.payload));
  appWindow.listen("playerdata_update", (e: TauriEvent<PlayerDataStatus>) => refresh(e.payload));
  startSpotifyPolling();
}
function createPopup(popup: Popup) { _createPopup(popup, shown); }
function checkTimerInterval() {
  // Switched off or overlay hidden: drop everything, held value included.
  if (!prefs || !prefs.displayTimer || !shown) { hideTimer(); return; }
  if (!determineActivityType(currentActivity?.activityInfo?.activityModes)) {
    // Running until now means the activity just ended: keep the last value on
    // screen rather than blanking it mid-count.
    if (timerRunning) { freezeTimer(); return; }
    if (frozenMillis === null) { stopTimer(); timerElem.classList.add("hidden"); }
    return;
  }
  clearHold(); frozenMillis = null;
  timerElem.classList.remove("hidden");
  if (!timerRunning) { timerRunning = true; timerTick(); }
}
function stopTimer() { timerRunning = false; clearTimeout(timerTimeout); timerTimeout = null; }
function clearHold() { clearTimeout(holdTimeout); holdTimeout = null; }
function hideTimer() { clearHold(); frozenMillis = null; stopTimer(); timerElem.classList.add("hidden"); }
function renderTimer(millis: number) {
  timeElem.innerHTML = formatTime(millis); msElem.innerHTML = formatMillis(millis);
}
function freezeTimer() {
  // Against the start it was last counting from, not currentActivity.startDate
  // -- by the time an activity ends the backend has already moved that on to
  // whatever came next, usually orbit.
  frozenMillis = Date.now() + clockOffsetMillis - runningStartMillis;
  stopTimer();
  renderTimer(frozenMillis);
  clearHold();
  holdTimeout = setTimeout(hideTimer, HOLD_AWAITING_API_MS);
}
// Replaces the held value with the run's official duration once it lands in
// the activity history.
function showApiTime(durationSeconds: number) {
  if (frozenMillis === null) return;
  frozenMillis = durationSeconds * 1000;
  renderTimer(frozenMillis);
  clearHold();
  holdTimeout = setTimeout(hideTimer, HOLD_AFTER_API_MS);
}
function refresh(playerDataStatus: PlayerDataStatus) {
  clockOffsetMillis = playerDataStatus?.clockOffsetMillis ?? 0;
  const playerData = playerDataStatus?.lastUpdate;
  if (!playerData) { widgetContentElem.classList.add("hidden"); currentActivity = null as any; doneInitialRefresh = false; hideTimer(); return; }
  loaderElem.classList.add("hidden"); widgetContentElem.classList.remove("hidden");
  currentActivity = playerData.currentActivity; checkTimerInterval();
  dailyElem.innerText = String(countClearsSince(playerData.activityHistory, destinyDailyReset()));
  weeklyElem.innerText = String(countClearsSince(playerData.activityHistory, destinyWeeklyReset()));
  const latestRaid = playerData.activityHistory[0];
  if (doneInitialRefresh && latestRaid && lastRaidId != latestRaid.instanceId) {
    const type = determineActivityType(latestRaid.modes);
    // Snapped for every finished run, cleared or not, and regardless of the
    // popup preference. Bungie's activityDurationSeconds is the activity's own
    // lifetime, which is the number the PGCR and every leaderboard quotes --
    // on a run left early it runs about 32s past the point you left, because
    // the instance stays alive that long after. Matching it is the whole point.
    if (type) showApiTime(latestRaid.activityDurationSeconds);
    // The popup announces a *clear*, so that one stays gated on completion.
    if (type && latestRaid.completed && prefs.displayClearNotifications) createPopup({ title: `${type.charAt(0).toUpperCase()+type.slice(1)} clear result`, subtext: `API Time: <strong>${latestRaid.activityDuration}</strong>` });
  }
  lastRaidId = latestRaid?.instanceId;
  doneInitialRefresh = true;
}
function applyPreferences(p: Preferences) {
  prefs = p;
  const root = document.documentElement;
  root.style.setProperty("--daily-color", p.dailyColor);
  root.style.setProperty("--weekly-color", p.weeklyColor);
  root.style.setProperty("--spotify-accent-color", p.spotifyAccentColor);
  root.style.setProperty("--overlay-text-color", p.overlayTextColor);
  root.style.setProperty("--overlay-scale", String(Math.min(150, Math.max(75, p.overlayScale ?? 100)) / 100));
  root.style.setProperty("--overlay-opacity", String(Math.min(100, Math.max(20, p.overlayOpacity ?? 100)) / 100));
  const position = p.overlayPosition ?? "top-right";
  const offsetX = Math.max(0, p.overlayOffsetX ?? 22);
  const offsetY = Math.max(0, p.overlayOffsetY ?? 18);

  const isLeft = position.endsWith("left");
  const isTop = position.startsWith("top");

  root.style.setProperty("--overlay-left", isLeft ? `${offsetX}px` : "auto");
  root.style.setProperty("--overlay-right", isLeft ? "auto" : `${offsetX}px`);
  root.style.setProperty("--overlay-top", isTop ? `${offsetY}px` : "auto");
  root.style.setProperty("--overlay-bottom", isTop ? "auto" : `${offsetY}px`);
  root.style.setProperty("--overlay-transform-origin", `${isTop ? "top" : "bottom"} ${isLeft ? "left" : "right"}`);
  timerElem.classList.toggle("section-disabled", !p.displayTimer);
  dailyRow.classList.toggle("hidden", !p.displayDailyClears);
  weeklyElem.classList.toggle("hidden", !p.displayWeeklyClears);
  spotifyElem.classList.toggle("section-disabled", !p.displaySpotify);
  msElem.classList.toggle("hidden", !p.displayMilliseconds);
  stopTimer(); checkTimerInterval();
}
function timerTick() {
  // A queued animation frame can outlive stopTimer(), by which point there may
  // be no activity left to time.
  if (!timerRunning) return;
  runningStartMillis = Number(new Date(currentActivity.startDate));
  const millis = Date.now() + clockOffsetMillis - runningStartMillis;
  renderTimer(millis);
  // Re-arm on the next instant the display actually changes rather than on a
  // fixed interval, so the seconds digit flips on the second instead of up to
  // half a tick after it.
  const step = prefs.displayMilliseconds ? 1000 / 30 : 1000;
  const delay = step - (((millis % step) + step) % step);
  timerTimeout = setTimeout(() => requestAnimationFrame(timerTick), delay);
}
function startSpotifyPolling() {
  const poll = async () => {
    try {
      const r = await fetch("http://127.0.0.1:8974/now-playing", { cache: "no-store" });
      if (!r.ok) throw new Error("spotify companion unavailable");
      const data = await r.json();
      if (!prefs?.displaySpotify || !data?.isPlaying || !data?.track) { spotifyElem.classList.add("hidden"); return; }
      spotifyTrack.textContent = data.track;
      spotifyArtist.textContent = data.artist || "";
      spotifyElem.classList.remove("hidden");
    } catch { spotifyElem.classList.add("hidden"); }
  };
  poll();
  spotifyInterval = setInterval(poll, 3000);
}
init();
