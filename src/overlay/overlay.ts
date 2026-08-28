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
let timerInterval: any;
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
    clearInterval(timerInterval); timerInterval = null; timerElem.classList.add("hidden"); return;
  }
  timerElem.classList.remove("hidden");
  if (!timerInterval) timerInterval = setInterval(() => requestAnimationFrame(timerTick), 1000 / (prefs.displayMilliseconds ? 30 : 2));
}
function refresh(playerDataStatus: PlayerDataStatus) {
  const playerData = playerDataStatus?.lastUpdate;
  if (!playerData) { widgetContentElem.classList.add("hidden"); currentActivity = null as any; doneInitialRefresh = false; return; }
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
  clearInterval(timerInterval); timerInterval = null; checkTimerInterval();
}
function timerTick() {
  const millis = Number(new Date()) - Number(new Date(currentActivity.startDate));
  timeElem.innerHTML = formatTime(millis); msElem.innerHTML = formatMillis(millis);
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
