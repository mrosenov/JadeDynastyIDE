/** A stored 0..1 probability shown as a percentage. */
export function formatProbability(value: number): string {
  const percent = value * 100;
  const absolute = Math.abs(percent);
  if (absolute > 0 && absolute < 0.000001) return `${percent.toExponential(2)}%`;
  const maximumFractionDigits = absolute >= 10 ? 2 : absolute >= 1 ? 3 : absolute >= 0.01 ? 4 : 6;
  return `${percent.toLocaleString(undefined, { maximumFractionDigits })}%`;
}

/** Turns an explicitly percent-suffixed value into the 0..1 value stored in the file. */
export function parseProbability(text: string): number | null {
  const trimmed = text.trim();
  if (!trimmed.endsWith("%")) return null;
  const digits = trimmed.slice(0, -1).trim();
  if (!digits || !/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:e[+-]?\d+)?$/i.test(digits)) return null;
  const number = Number(digits);
  return Number.isFinite(number) ? number / 100 : null;
}
