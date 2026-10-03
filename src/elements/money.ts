// Money values (the "money" role): 1 = 1 Copper, 100 Copper = 1 Silver, 100 Silver = 1 Gold.

export interface Coins {
  gold: number;
  silver: number;
  copper: number;
  negative: boolean;
}

export function coins(value: number): Coins {
  const v = Math.abs(Math.trunc(value));
  return { gold: Math.floor(v / 10_000), silver: Math.floor(v / 100) % 100, copper: v % 100, negative: value < 0 };
}

/** "12G 34S 56C", leaving out zero parts ("5S", "0C"). */
export function formatMoney(value: number): string {
  const c = coins(value);
  const parts = [c.gold && `${c.gold.toLocaleString()}G`, c.silver && `${c.silver}S`, c.copper && `${c.copper}C`].filter(Boolean);
  return (c.negative ? "−" : "") + (parts.length ? parts.join(" ") : "0C");
}

/** "12 Gold 34 Silver 56 Copper", for tooltips. */
export function formatMoneyWords(value: number): string {
  const c = coins(value);
  const parts = [c.gold && `${c.gold.toLocaleString()} Gold`, c.silver && `${c.silver} Silver`, c.copper && `${c.copper} Copper`].filter(Boolean);
  return (c.negative ? "−" : "") + (parts.length ? parts.join(" ") : "0 Copper");
}

/**
 * Copper from "12G 34S 56C" or "1 gold 50 silver" (any of the parts, any
 * order, any case, spaces optional), or null when the text is not written that
 * way (plain numbers stay as they are).
 */
export function parseMoney(text: string): number | null {
  const t = text.trim().toLowerCase();
  if (!/^-?(\s*\d+\s*(gold|silver|copper|g|s|c))+$/.test(t)) return null;
  const sign = t.startsWith("-") ? -1 : 1;
  let total = 0;
  for (const [, n, unit] of t.matchAll(/(\d+)\s*(gold|silver|copper|g|s|c)/g)) {
    total += Number(n) * (unit.startsWith("g") ? 10_000 : unit.startsWith("s") ? 100 : 1);
  }
  return sign * total;
}
