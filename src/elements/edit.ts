// Editing helpers: which fields can be edited, and what a changed field held.

import type { FieldNode } from "./types";

/** A value the inspector can edit in place: a leaf with a value. */
export const isEditable = (node: FieldNode) => !node.children && !node.group && node.value !== undefined;

export const INTEGER_TYPES = new Set(["int8", "uint8", "int16", "uint16", "int32", "uint32", "int64", "uint64"]);
export const FLOAT_TYPES = new Set(["float", "double"]);

/** A number field that the inspector's multi-field quick editor can change. */
export const isNumericField = (node: FieldNode) => isEditable(node) && (INTEGER_TYPES.has(node.ty) || FLOAT_TYPES.has(node.ty));

/** Whether a field's bytes differ from the record as the file was opened. */
export function isChanged(node: FieldNode, bytes: number[], original?: number[]): boolean {
  if (!original) return false;
  for (let i = node.off; i < node.off + node.size; i++) if (bytes[i] !== original[i]) return true;
  return false;
}

/** Full capacity of a fixed-size text field. Full fields have no terminator. */
export function textCapacity(ty: string): { max: number; unit: "characters" | "bytes" } | null {
  const w = /^wchar\[(\d+)\]$/.exec(ty);
  if (w) return { max: Number(w[1]), unit: "characters" };
  const c = /^char\[(\d+)\]$/.exec(ty);
  if (c) return { max: Number(c[1]), unit: "bytes" };
  return null;
}

/** Length of a text as the field stores it: UTF-16 units, or GBK bytes (CJK takes 2). */
export function textLength(text: string, unit: "characters" | "bytes"): number {
  const crlf = text.replace(/\r?\n/g, "\r\n");
  if (unit === "characters") return crlf.length;
  let n = 0;
  for (const ch of crlf) n += ch.charCodeAt(0) < 0x80 ? 1 : 2;
  return n;
}

const gbk = new TextDecoder("gbk");

/** A field's value in other bytes (the original record), as the tree shows values. */
export function decodeValue(ty: string, bytes: number[], off: number): string | null {
  const b = Uint8Array.from(bytes);
  const v = new DataView(b.buffer);
  try {
    switch (ty) {
      case "int8":
        return String(v.getInt8(off));
      case "uint8":
        return String(v.getUint8(off));
      case "bool":
        return String(v.getUint8(off) !== 0);
      case "int16":
        return String(v.getInt16(off, true));
      case "uint16":
        return String(v.getUint16(off, true));
      case "int32":
        return String(v.getInt32(off, true));
      case "uint32":
        return String(v.getUint32(off, true));
      case "int64":
        return v.getBigInt64(off, true).toString();
      case "uint64":
        return v.getBigUint64(off, true).toString();
      case "float":
        return String(Number(v.getFloat32(off, true).toPrecision(7)));
      case "double":
        return String(v.getFloat64(off, true));
    }
    const w = /^wchar\[(\d+)\]$/.exec(ty);
    if (w) {
      const units: number[] = [];
      for (let i = 0; i < Number(w[1]); i++) {
        const u = v.getUint16(off + i * 2, true);
        if (u === 0) break;
        units.push(u);
      }
      return String.fromCharCode(...units);
    }
    const c = /^char\[(\d+)\]$/.exec(ty);
    if (c) {
      const raw = b.subarray(off, off + Number(c[1]));
      const end = raw.indexOf(0);
      return gbk.decode(end < 0 ? raw : raw.subarray(0, end));
    }
    const h = /^byte\[(\d+)\]$/.exec(ty);
    if (h) return [...b.subarray(off, off + Number(h[1]))].map((x) => x.toString(16).padStart(2, "0")).join(" ");
  } catch {
    return null;
  }
  return null;
}
