// Field lists as other editors write them (sELedit, Jade Editor .cfg):
//
//   ID;Name;Type;Count;Value_1;Value_2;Value_3
//   int32;wstring:64;int32;int32;float;float;float
//
// a line of names and a line of types, separated by ";" (or tabs, or
// commas). A whole .cfg block works too: its "001 - EQUIPMENT_ADDON" line
// names the struct and the other lines are skipped. Sizes in "wstring:N",
// "string:N" and "byte:N" are bytes; "byte:AUTO" takes the rest of the record.

import { type EditField, type Kind, fieldSize, kindInfo, newField, renameDuplicates } from "./model";

export interface ParsedField {
  name: string;
  /** The type as written. */
  type: string;
  /** Byte offset in the record. */
  off: number;
  size: number;
  field?: EditField;
  error?: string;
}

export interface FieldList {
  fields: ParsedField[];
  /** Struct name from a "001 - NAME" line. */
  struct?: string;
  /** What does not fit as a whole (nothing can be imported). */
  errors: string[];
  /** Guesses made: missing names, extra names. */
  notes: string[];
  /** Bytes the fields take. */
  size: number;
}

const SIMPLE: Record<string, Kind> = {
  int32: "i32",
  int: "i32",
  uint32: "u32",
  uint: "u32",
  dword: "u32",
  float: "f32",
  double: "f64",
  int64: "i64",
  uint64: "u64",
  int16: "i16",
  short: "i16",
  uint16: "u16",
  ushort: "u16",
  word: "u16",
  int8: "i8",
  char: "i8",
  uint8: "u8",
  byte: "u8",
};

const SIZED: Record<string, Kind> = { wstring: "wstr", string: "str", byte: "bytes", bytes: "bytes" };

/** The cells of a line: split at ";", else tabs, else commas; a trailing empty cell dropped. */
function cells(line: string): string[] {
  const sep = line.includes(";") ? ";" : line.includes("\t") ? "\t" : ",";
  const out = line.split(sep).map((c) => c.trim());
  if (out.length > 1 && out[out.length - 1] === "") out.pop();
  return out;
}

type TypeResult = { kind: Kind; len: number; dims: number[]; auto?: boolean } | { error: string };

/** "int32", "wstring:64", "byte:AUTO", "float[3]" → a field type. */
function parseType(text: string): TypeResult {
  const m = /^([a-z0-9]+)(?::(\d+|auto))?((?:\[\d+\])*)$/i.exec(text.replace(/\s+/g, ""));
  if (!m) return { error: text ? `“${text}” is not a type` : "No type" };
  const [, base, param, arrays] = m;
  const dims = [...arrays.matchAll(/\[(\d+)\]/g)].map((d) => Number(d[1]));
  if (dims.some((d) => d < 1)) return { error: "Array sizes must be at least 1" };
  const name = base.toLowerCase();
  if (param === undefined) {
    const kind = SIMPLE[name];
    return kind ? { kind, len: 0, dims } : { error: SIZED[name] ? `${base} needs a size, e.g. ${base}:32` : `Unknown type “${base}”` };
  }
  const kind = SIZED[name];
  if (!kind) return { error: `${base} takes no size` };
  if (param.toLowerCase() === "auto") {
    return kind === "bytes" && dims.length === 0 ? { kind, len: 0, dims, auto: true } : { error: "Only byte:AUTO takes the rest of the record" };
  }
  const bytes = Number(param);
  if (bytes < 1) return { error: "The size must be at least 1 byte" };
  if (kind === "wstr" && bytes % 2) return { error: "wstring sizes are bytes, two per character: use an even number" };
  return { kind, len: kind === "wstr" ? bytes / 2 : bytes, dims };
}

const looksLikeTypes = (line: string) => {
  const c = cells(line);
  return c.length > 0 && c.filter((t) => !("error" in parseType(t))).length * 2 >= c.length;
};

/** Reads a pasted field list against a record of `recordSize` bytes. */
export function parseFieldList(text: string, recordSize: number): FieldList {
  const lines = text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith("//"));
  const errors: string[] = [];
  const notes: string[] = [];
  const struct = lines.map((l) => /^\d+\s*-\s*([A-Za-z_]\w*)\s*$/.exec(l)?.[1]).find(Boolean);
  // The types are the last line that reads as types; the names come right before.
  let at = -1;
  for (let i = lines.length - 1; i >= 0; i--) {
    if (looksLikeTypes(lines[i])) {
      at = i;
      break;
    }
  }
  // Without a clear one, the last line (its cells then show what is wrong).
  if (at < 0 && lines.length >= 2) at = lines.length - 1;
  if (!lines.length) return { fields: [], struct, errors: [], notes, size: 0 };
  if (at < 0) return { fields: [], struct, errors: ["No line of types found (e.g. int32;wstring:64;float)."], notes, size: 0 };
  const types = cells(lines[at]);
  const names = at > 0 && !looksLikeTypes(lines[at - 1]) ? cells(lines[at - 1]) : [];
  if (!names.length) notes.push("No line of names before the types: the fields are named field_1, field_2, …");
  else if (names.length < types.length) notes.push(`${names.length} names for ${types.length} types: the rest are named field_N.`);
  else if (names.length > types.length) notes.push(`${names.length} names for ${types.length} types: the last ${names.length - types.length} names are left out.`);

  const parsed = types.map((type, i) => ({ type, name: names[i] || `field_${i + 1}`, result: parseType(type) }));
  const autos = parsed.filter((p) => "auto" in p.result && p.result.auto);
  const fixed = parsed.reduce((sum, p) => {
    if ("error" in p.result || p.result.auto) return sum;
    return sum + fieldSize(newField({ kind: p.result.kind, len: p.result.len, dims: p.result.dims }));
  }, 0);

  let off = 0;
  const fields: ParsedField[] = parsed.map((p) => {
    if ("error" in p.result) return { name: p.name, type: p.type, off, size: 0, error: p.result.error };
    let { len } = p.result;
    if (p.result.auto) {
      len = recordSize - fixed;
      if (autos.length > 1) return { name: p.name, type: p.type, off, size: 0, error: "Only one byte:AUTO can take the rest of the record" };
      if (len < 1) return { name: p.name, type: p.type, off, size: 0, error: "No bytes are left for byte:AUTO" };
    }
    const field = newField({ name: p.name, kind: p.result.kind, len, dims: p.result.dims });
    const size = fieldSize(field);
    const out = { name: p.name, type: p.type, off, size, field };
    off += size;
    return out;
  });
  return { fields, struct, errors, notes, size: off };
}

/** The fields to put in the draft (duplicate names numbered). */
export const listFields = (list: FieldList): EditField[] => renameDuplicates(list.fields.flatMap((f) => (f.field ? [f.field] : [])));

/** "wide string · 32 chars" and the like, for the preview. */
export function describe(f: EditField): string {
  const info = kindInfo(f.kind);
  const base = f.kind === "wstr" ? `${info.label} · ${f.len} chars` : info.len ? `${info.label} · ${f.len} B` : info.label;
  return f.dims.length ? `${base} [${f.dims.join("×")}]` : base;
}
