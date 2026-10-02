// Possible readings of undefined record bytes, to help pick a field type.

import type { Kind } from "../schema/model";
import type { FieldNode } from "./types";
import { formatDuration, formatUnix, plausibleUnix } from "./time";

export interface Reading {
  key: string;
  label: string;
  value: string;
  /** Extra detail, e.g. the byte length of a text reading. */
  detail?: string;
  /** The value looks plausible for this type. */
  likely: boolean;
  /** Field to define for this reading. */
  define: { kind: Kind; len?: number; dims?: number[]; display?: string; size: number };
}

/** Bytes the selection can be read from: [start, end) of the undefined run. */
export interface Span {
  start: number;
  end: number;
}

const placeholderName = (name: string) => /^unknown_[0-9a-f]+$/i.test(name) || name.startsWith("_pad");

/** A node whose bytes are not described yet: a gap, or a placeholder field. */
export const isUndefinedNode = (node: FieldNode) => !!node.unknown || (!node.children && placeholderName(node.name));

/**
 * The run of undefined bytes around `offset`: the top-level gap or
 * placeholder holding it, merged with adjacent ones.
 */
export function undefinedSpan(nodes: FieldNode[], offset: number): Span | null {
  // Top-level order, with groups opened up (their members sit inline).
  const flat = nodes.flatMap((n) => (n.group ? (n.children ?? []) : [n]));
  const i = flat.findIndex((n) => offset >= n.off && offset < n.off + n.size);
  if (i < 0 || !isUndefinedNode(flat[i])) return null;
  let start = flat[i].off;
  let end = flat[i].off + flat[i].size;
  for (let j = i - 1; j >= 0 && isUndefinedNode(flat[j]) && flat[j].off + flat[j].size === start; j--) start = flat[j].off;
  for (let j = i + 1; j < flat.length && isUndefinedNode(flat[j]) && flat[j].off === end; j++) end += flat[j].size;
  return { start, end };
}

const printable = (c: number) => (c >= 0x20 && c !== 0x7f && c < 0xd800) || (c >= 0xe000 && c < 0xfffe);

const plausibleFloat = (f: number, bits: number) =>
  Number.isFinite(f) && f !== 0 && Math.abs(f) > 1e-4 && Math.abs(f) < 1e7 && (bits & 0x7f800000) !== 0;

const fmtFloat = (f: number) => (Number.isFinite(f) ? String(Number(f.toPrecision(7))) : String(f));

/**
 * Byte length to define for a text field: the largest common array size that
 * holds the text and its terminator and stays within the zero padding after
 * it (those zeros may also belong to the next field), else text plus padding.
 */
function fieldLength(toNul: number, padded: number, room: number): number {
  const limit = Math.min(padded, room);
  const sizes = [16, 32, 64, 128, 256, 512].filter((n) => n >= toNul && n <= limit);
  return sizes.length ? sizes[sizes.length - 1] : Math.max(toNul, limit);
}

/** Bytes after a text terminator that are still zero, up to `limit`. */
function zeroRun(bytes: Uint8Array, from: number, limit: number, step: number) {
  let end = from;
  while (end + step <= limit && bytes.subarray(end, end + step).every((b) => b === 0)) end += step;
  return end;
}

export function readings(raw: number[], offset: number, span: Span): Reading[] {
  const bytes = Uint8Array.from(raw);
  const view = new DataView(bytes.buffer);
  const room = bytes.length - offset;
  const spanRoom = span.end - offset;
  const out: Reading[] = [];

  if (room >= 4) {
    const i32 = view.getInt32(offset, true);
    const u32 = view.getUint32(offset, true);
    const f32 = view.getFloat32(offset, true);
    const lo = view.getInt16(offset, true);
    const hi = view.getInt16(offset + 2, true);
    const floatLike = plausibleFloat(f32, u32);
    out.push({
      key: "i32",
      label: "int32",
      value: String(i32),
      likely: !floatLike && Math.abs(i32) < 10_000_000,
      define: { kind: "i32", size: 4 },
    });
    out.push({
      key: "u32",
      label: "uint32",
      value: String(u32),
      likely: false,
      define: { kind: "u32", size: 4 },
    });
    out.push({
      key: "i16x2",
      label: "int16 × 2",
      value: `${lo}, ${hi}`,
      likely: hi !== 0 && Math.abs(lo) < 10_000 && Math.abs(hi) < 10_000 && !floatLike,
      define: { kind: "i16", dims: [2], size: 4 },
    });
    if (plausibleUnix(u32)) {
      out.push({
        key: "time",
        label: "unix time",
        value: formatUnix(u32) ?? String(u32),
        likely: true,
        define: { kind: "i32", display: "time", size: 4 },
      });
    }
    // Whole minutes up to a year read well as a duration.
    if (u32 >= 60 && u32 <= 31_536_000 && u32 % 60 === 0) {
      out.push({
        key: "duration",
        label: "duration",
        value: formatDuration(u32),
        likely: false,
        define: { kind: "i32", display: "duration", size: 4 },
      });
    }
    out.push({
      key: "f32",
      label: "float",
      value: fmtFloat(f32),
      likely: floatLike,
      define: { kind: "f32", size: 4 },
    });
  } else if (room >= 2) {
    const v = view.getInt16(offset, true);
    out.push({ key: "i16", label: "int16", value: String(v), likely: true, define: { kind: "i16", size: 2 } });
  }
  if (room >= 1 && room < 4) {
    out.push({
      key: "u8",
      label: "uint8",
      value: String(bytes[offset]),
      likely: room === 1,
      define: { kind: "u8", size: 1 },
    });
  }
  if (room >= 8) {
    const f64 = view.getFloat64(offset, true);
    const i64 = view.getBigInt64(offset, true);
    out.push({
      key: "i64",
      label: "int64",
      value: i64.toString(),
      likely: false,
      define: { kind: "i64", size: 8 },
    });
    const doubleLike = Number.isFinite(f64) && f64 !== 0 && Math.abs(f64) > 1e-6 && Math.abs(f64) < 1e12;
    if (doubleLike) {
      out.push({ key: "f64", label: "double", value: fmtFloat(f64), likely: true, define: { kind: "f64", size: 8 } });
    }
  }

  // UTF-16 text up to its terminator, sized to include the zero padding.
  if (room >= 2) {
    let p = offset;
    const units: number[] = [];
    while (p + 2 <= bytes.length && units.length < 512) {
      const u = view.getUint16(p, true);
      p += 2;
      if (u === 0) break;
      units.push(u);
    }
    if (units.length) {
      const toNul = units.length * 2 + 2;
      const padded = Math.max(toNul, zeroRun(bytes, offset + toNul, span.end, 2) - offset);
      const shown = String.fromCharCode(...units.slice(0, 48)) + (units.length > 48 ? "…" : "");
      const allPrintable = units.every(printable);
      const fits = toNul <= spanRoom;
      const length = fieldLength(toNul, padded, spanRoom) & ~1;
      out.push({
        key: "wstr",
        label: "text utf-16",
        value: `“${shown.replace(/[\u0000-\u001f]/g, "·")}”`,
        detail: `${toNul} bytes` + (length > toNul ? ` · as wchar[${length >> 1}] (${length} B)` : ""),
        likely: fits && units.length >= 2 && allPrintable && offset % 2 === 0,
        define: { kind: "wstr", len: Math.max(1, length >> 1), size: Math.max(2, length) },
      });
    }
  }

  // GBK text (char arrays: file paths, Chinese names).
  if (room >= 1 && bytes[offset] !== 0) {
    let end = offset;
    while (end < bytes.length && bytes[end] !== 0 && end - offset < 512) end++;
    const toNul = end - offset + 1;
    const padded = Math.max(toNul, zeroRun(bytes, end + 1, span.end, 1) - offset);
    const length = fieldLength(toNul, padded, spanRoom);
    let text = "";
    let clean = false;
    try {
      text = new TextDecoder("gbk", { fatal: true }).decode(bytes.subarray(offset, end));
      clean = [...text].every((c) => printable(c.codePointAt(0)!));
    } catch {
      text = new TextDecoder("gbk").decode(bytes.subarray(offset, end));
    }
    out.push({
      key: "str",
      label: "text gbk",
      value: `“${text.slice(0, 48).replace(/[\u0000-\u001f]/g, "·")}${text.length > 48 ? "…" : ""}”`,
      detail: `${toNul} bytes` + (length > toNul ? ` · as char[${length}]` : ""),
      likely: clean && end - offset >= 3 && toNul <= spanRoom,
      define: { kind: "str", len: Math.max(1, length), size: Math.max(1, length) },
    });
  }

  return out;
}

/** True when the bytes at the offset are all zero, so every number reads 0. */
export const allZero = (raw: number[], offset: number, length: number) =>
  raw.slice(offset, offset + length).every((b) => b === 0);
