import { ACTIVITY_TYPES } from "./consts";
import type { CompletedActivity } from "./types";

// Clamped, because clock-offset correction can briefly put the activity start
// a few milliseconds in the future, which would otherwise render as "0:-1".
export function formatTime(millis: number): string {
  let seconds = Math.floor(Math.max(0, millis) / 1000);
  let minutes = Math.floor(seconds / 60);
  seconds -= minutes * 60;
  const hours = Math.floor(minutes / 60);
  minutes -= hours * 60;
  return (hours > 0 ? `${hours}:` : "") + String(minutes).padStart(2, "0") + ":" + String(seconds).padStart(2, "0");
}
export function formatMillis(millis: number): string {
  return ":" + String(Math.max(0, millis) % 1000).padStart(3, "0").substring(0, 2);
}
export function countClears(activityHistory: CompletedActivity[]): number {
  return activityHistory.filter(a => a.completed).length;
}
export function countClearsSince(activityHistory: CompletedActivity[], cutoff: Date): number {
  return activityHistory.filter(a => a.completed && new Date(a.period) >= cutoff).length;
}
export function destinyDailyReset(now = new Date()): Date {
  const cutoff = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate(), 17, 0, 0));
  if (cutoff > now) cutoff.setUTCDate(cutoff.getUTCDate() - 1);
  return cutoff;
}
export function destinyWeeklyReset(now = new Date()): Date {
  const daily = destinyDailyReset(now);
  const day = daily.getUTCDay(); // Sun=0, Tue=2
  const daysSinceTuesday = (day - 2 + 7) % 7;
  const cutoff = new Date(daily);
  cutoff.setUTCDate(cutoff.getUTCDate() - daysSinceTuesday);
  return cutoff;
}
export function determineActivityType(modes: number[]): string | undefined {
  if (!modes) return;
  for (const mode of modes) if (ACTIVITY_TYPES[mode]) return ACTIVITY_TYPES[mode];
}
