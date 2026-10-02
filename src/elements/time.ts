// Times for fields with a time role: unix dates ("time"), durations in seconds
// or milliseconds ("duration", "duration_ms") and times of day ("daytime").

/** Earliest and latest seconds treated as a plausible date (2000–2040). */
export const PLAUSIBLE_FROM = 946_684_800;
export const PLAUSIBLE_TO = 2_208_988_800;

const pad = (n: number) => String(n).padStart(2, "0");

/** Local date and time, e.g. "2014-03-28 12:26:34"; null for 0 or out-of-range values. */
export function formatUnix(seconds: number): string | null {
  if (!Number.isFinite(seconds) || seconds <= 0 || seconds > 32_503_680_000) return null;
  const d = new Date(seconds * 1000);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

/** The same moment in UTC, ISO form, for tooltips. */
export const formatUnixUtc = (seconds: number) => new Date(seconds * 1000).toISOString().replace("T", " ").replace(".000Z", " UTC");

export const plausibleUnix = (seconds: number) => seconds >= PLAUSIBLE_FROM && seconds <= PLAUSIBLE_TO;

// ---------------------------------------------------------------- durations and times of day

/** Roles whose values are times: shown as a chip instead of a float reading. */
export const TIME_ROLES = new Set(["time", "duration", "duration_ms", "daytime"]);

const UNITS: [number, string][] = [
  [86_400, "d"],
  [3_600, "h"],
  [60, "m"],
  [1, "s"],
];

/**
 * A length of time, e.g. "1h 30m", "7d", "45s", "1.5s", "250 ms"; at most
 * the two largest units, as a game designer would say it.
 */
export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds)) return String(seconds);
  const sign = seconds < 0 ? "-" : "";
  let rest = Math.abs(seconds);
  if (rest === 0) return "0s";
  if (rest < 1) return `${sign}${Math.round(rest * 1000)} ms`;
  if (rest < 60 && !Number.isInteger(rest)) return `${sign}${Number(rest.toFixed(2))}s`;
  rest = Math.round(rest);
  const parts: string[] = [];
  for (const [size, unit] of UNITS) {
    if (rest >= size && parts.length < 2) {
      parts.push(`${Math.floor(rest / size)}${unit}`);
      rest %= size;
    } else if (parts.length) {
      // Units after the largest are consecutive: "1d 3h", never "1d 3m".
      break;
    }
  }
  return sign + parts.join(" ");
}

/** Seconds since midnight as "14:30" (or "14:30:15"); null outside one day. */
export function formatDaytime(seconds: number): string | null {
  if (!Number.isInteger(seconds) || seconds < 0 || seconds > 86_400) return null;
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  return `${pad(h)}:${pad(m)}` + (s ? `:${pad(s)}` : "");
}
