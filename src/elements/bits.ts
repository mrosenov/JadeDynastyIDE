// Bit maths for masks, with BigInt so 64-bit masks keep every bit.

/** Bit width of a decoded field type label ("int32", "uint64", …). */
export function bitWidth(ty: string): number {
  if (/^u?int8$|^bool$/.test(ty)) return 8;
  if (/^u?int16$/.test(ty)) return 16;
  if (/^u?int64$/.test(ty)) return 64;
  return 32;
}

const mask = (width: number) => (1n << BigInt(width)) - 1n;

/** A decimal value (possibly negative) as its unsigned bit pattern. */
export function toBits(value: string | number, width: number): bigint {
  try {
    return BigInt(value) & mask(width);
  } catch {
    return 0n;
  }
}

/** The bit pattern read as a signed number of the given width. */
export function toSigned(bits: bigint, width: number): bigint {
  const sign = 1n << BigInt(width - 1);
  return bits & sign ? bits - (1n << BigInt(width)) : bits;
}

export const bitValue = (bit: number) => 1n << BigInt(bit);

export const hasBit = (bits: bigint, bit: number) => (bits & bitValue(bit)) !== 0n;

export const hex = (bits: bigint, width = 32) => "0x" + bits.toString(16).toUpperCase().padStart(Math.ceil(width / 4), "0");

/** Value of a bit as hex, e.g. bit 3 → "0x8". */
export const bitHex = (bit: number) => "0x" + bitValue(bit).toString(16).toUpperCase();

/** Lowercase key from a label: "Trade Behavior" → "trade_behavior". */
export const slug = (text: string) =>
  text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
