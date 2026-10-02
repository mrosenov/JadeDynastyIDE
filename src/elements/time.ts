// Unix timestamps (seconds since 1970-01-01 UTC) for fields with the "time" role.

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
