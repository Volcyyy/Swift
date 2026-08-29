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
  if (!prefs || !prefs.displayTimer || !shown || !determineActivityType(currentActivity?.activityInfo?.activityModes)) {
    stopTimer(); timerElem.classList.add("hidden"); return;
  }
  timerElem.classList.remove("hidden");
  if (!timerRunning) { timerRunning = true; timerTick(); }
}
function stopTimer() { timerRunning = false; clearTimeout(timerTimeout); timerTimeout = null; }
function refresh(playerDataStatus: PlayerDataStatus) {
  clockOffsetMillis = playerDataStatus?.clockOffsetMillis ?? 0;
  const playerData = playerDataStatus?.lastUpdate;
  if (!playerData) { widgetContentElem.classList.add("hidden"); currentActivity = null as any; doneInitialRefresh = false; stopTimer(); timerElem.classList.add("hidden"); return; }
  loaderElem.classList.add("hidden"); widgetContentElem.classList.remove("hidden");
  currentActivity = playerData.currentActivity; checkTimerInterval();
  dailyElem.innerText = String(countClearsSince(playerData.activityHistory, destinyDailyReset()));
  weeklyElem.innerText = String(countClearsSince(playerData.activityHistory, destinyWeeklyReset()));
  const latestRaid = playerData.activityHistory[0];
  if (doneInitialRefresh && latestRaid?.completed && lastRaidId != latestRaid.instanceId && prefs.displayClearNotifications) {
    const type = determineActivityType(latestRaid.modes);
    if (type) createPopup({ title: `${type.charAt(0).toUpperCase()+type.slice(1)} clear result`, subtext: `API Time: <strong>${latestRaid.activityDuration}</strong>` });
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
  const millis = Date.now() + clockOffsetMillis - Number(new Date(currentActivity.startDate));
  timeElem.innerHTML = formatTime(millis); msElem.innerHTML = formatMillis(millis);
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
