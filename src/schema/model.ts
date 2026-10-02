// The schema editor's model: a tree of fields laid out back to back.
//
// Stored definitions carry explicit offsets (so alignment gaps are implicit)
// and nest arrays as types. The editor instead lists fields in order, each
// with an optional array shape, and turns gaps into explicit "_pad" byte
// fields so a definition converts to the editor and back unchanged.
//
// Groups are editor rows that hold consecutive fields of the same level for
// display only. They are saved as a "g" attribute on each member, so they
// never change field names or offsets.

import type { Field, ListDef, Ty } from "../elements/types";

export type Kind =
  | "i8"
  | "u8"
  | "bool"
  | "i16"
  | "u16"
  | "i32"
  | "u32"
  | "f32"
  | "f64"
  | "i64"
  | "u64"
  | "wstr"
  | "str"
  | "bytes"
  | "struct"
  | "group";

export interface KindInfo {
  kind: Kind;
  label: string;
  /** Fixed size in bytes; absent when it depends on `len` or children. */
  size?: number;
  /** What `len` counts, for kinds that take one. */
  len?: "chars" | "bytes";
}

export const KINDS: KindInfo[] = [
  { kind: "i32", label: "int32", size: 4 },
  { kind: "u32", label: "uint32", size: 4 },
  { kind: "f32", label: "float", size: 4 },
  { kind: "i16", label: "int16", size: 2 },
  { kind: "u16", label: "uint16", size: 2 },
  { kind: "i8", label: "int8", size: 1 },
  { kind: "u8", label: "uint8", size: 1 },
  { kind: "bool", label: "bool", size: 1 },
  { kind: "i64", label: "int64", size: 8 },
  { kind: "u64", label: "uint64", size: 8 },
  { kind: "f64", label: "double", size: 8 },
  { kind: "wstr", label: "wide string", len: "chars" },
  { kind: "str", label: "string (GBK)", len: "bytes" },
  { kind: "bytes", label: "bytes", len: "bytes" },
  { kind: "struct", label: "struct" },
];

const GROUP_INFO: KindInfo = { kind: "group", label: "group" };

/** Kinds with members laid out inside them. */
export const hasMembers = (kind: Kind) => kind === "struct" || kind === "group";

export const kindInfo = (kind: Kind) => KINDS.find((k) => k.kind === kind) ?? GROUP_INFO;

export interface EditField {
  uid: number;
  name: string;
  kind: Kind;
  /** Characters (wstr) or bytes (str, bytes). */
  len: number;
  /** Array shape, outermost first; empty for a single value. */
  dims: number[];
  /** Members, for kind "struct". */
  children: EditField[];
  e: string;
  display: string;
  refs: string[];
  c: string;
  /** Conditional types, checked in order. */
  when: EditRule[];
}

/** "If `field` is (not) one of `values`, read this field as `kind`." */
export interface EditRule {
  uid: number;
  field: string;
  values: number[];
  not: boolean;
  kind: Kind;
}

export interface Draft {
  name: string;
  struct: string;
  key?: string;
  fields: EditField[];
}

let nextUid = 1;
export const uid = () => nextUid++;

export function newField(partial: Partial<EditField> = {}): EditField {
  return {
    uid: uid(),
    name: "",
    kind: "i32",
    len: 32,
    dims: [],
    children: [],
    e: "",
    display: "",
    refs: [],
    c: "",
    when: [],
    ...partial,
  };
}

export const PAD_PREFIX = "_pad";

/** Padding fields keep offsets but are not saved as fields. */
export const isPad = (f: EditField) => f.kind === "bytes" && f.name.startsWith(PAD_PREFIX);

const hex4 = (n: number) => n.toString(16).toUpperCase().padStart(4, "0");

function padField(off: number, size: number): EditField {
  return newField({ name: `${PAD_PREFIX}_${hex4(off)}`, kind: "bytes", len: size });
}

// ---------------------------------------------------------------- sizes

export function tySize(t: Ty): number {
  switch (t.k) {
    case "i8":
    case "u8":
    case "bool":
      return 1;
    case "i16":
    case "u16":
      return 2;
    case "i32":
    case "u32":
    case "f32":
      return 4;
    case "f64":
    case "i64":
    case "u64":
      return 8;
    case "wstr":
      return t.n * 2;
    case "str":
    case "bytes":
      return t.n;
    case "array":
      return t.n * t.stride;
    case "struct":
      return t.fields.reduce((max, f) => Math.max(max, f.off + tySize(f.t)), 0);
  }
}

/** Size of one element (ignoring the array shape). */
export function elemSize(f: EditField): number {
  const info = kindInfo(f.kind);
  if (info.size !== undefined) return info.size;
  if (f.kind === "wstr") return f.len * 2;
  if (hasMembers(f.kind)) return f.children.reduce((sum, c) => sum + fieldSize(c), 0);
  return f.len;
}

export const fieldSize = (f: EditField) => f.dims.reduce((size, d) => size * d, elemSize(f));

export const draftSize = (fields: EditField[]) => fields.reduce((sum, f) => sum + fieldSize(f), 0);

// ---------------------------------------------------------------- conversion

function fromTy(t: Ty, padTo?: number): Pick<EditField, "kind" | "len" | "dims" | "children"> {
  const dims: number[] = [];
  let paddedElem = padTo;
  while (t.k === "array") {
    dims.push(t.n);
    paddedElem = t.stride;
    t = t.t;
  }
  switch (t.k) {
    case "struct":
      return { kind: "struct", len: 0, dims, children: fromFields(t.fields, paddedElem) };
    case "wstr":
    case "str":
    case "bytes":
      return { kind: t.k, len: t.n, dims, children: [] };
    default:
      return { kind: t.k, len: 0, dims, children: [] };
  }
}

/** Stored fields → editor fields, with gaps (and a tail up to `size`) as pads. */
export function fromFields(fields: Field[], size?: number): EditField[] {
  const out: { field: EditField; g?: string }[] = [];
  let cursor = 0;
  let previous: string | undefined;
  for (const f of [...fields].sort((a, b) => a.off - b.off)) {
    if (f.off < cursor) continue; // overlapping fields (unions) cannot be laid out in order
    // A gap between two members of one group stays inside the group.
    if (f.off > cursor) out.push({ field: padField(cursor, f.off - cursor), g: previous === f.g ? f.g : undefined });
    previous = f.g;
    out.push({
      g: f.g,
      field: {
        uid: uid(),
        name: f.name,
        ...fromTy(f.t),
        e: f.e ?? "",
        display: f.display ?? "",
        refs: f.refs ?? [],
        c: f.c ?? "",
        when: (f.when ?? []).map((r) => ({ uid: uid(), field: r.field, values: r.in, not: !!r.not, kind: r.t.k as Kind })),
      },
    });
    cursor = f.off + tySize(f.t);
  }
  if (size !== undefined && size > cursor) out.push({ field: padField(cursor, size - cursor) });

  // Fold runs of the same group into group rows.
  const folded: EditField[] = [];
  let group: EditField | null = null;
  for (const { field, g } of out) {
    if (g && group?.name === g) {
      group.children.push(field);
    } else if (g) {
      group = newField({ name: g, kind: "group", children: [field] });
      folded.push(group);
    } else {
      group = null;
      folded.push(field);
    }
  }
  return folded;
}

export function fromDef(def: ListDef | null, fallbackName: string): Draft {
  return {
    name: def?.name ?? fallbackName,
    struct: def?.struct ?? "",
    key: def?.key,
    fields: fromFields(def?.fields ?? [], def?.size),
  };
}

function toTy(f: EditField): Ty {
  let t: Ty;
  switch (f.kind) {
    case "struct":
      t = { k: "struct", fields: toFields(f.children) };
      break;
    case "wstr":
    case "str":
    case "bytes":
      t = { k: f.kind, n: f.len };
      break;
    case "group":
      throw new Error("Groups are flattened by toFields and have no type");
    default:
      t = { k: f.kind };
  }
  let stride = elemSize(f);
  for (const n of [...f.dims].reverse()) {
    t = { k: "array", n, stride, t };
    stride *= n;
  }
  return t;
}

/** Editor fields → stored fields with offsets. Pads become gaps; group
 *  members are saved inline, tagged with their group's name. */
export function toFields(fields: EditField[]): Field[] {
  const out: Field[] = [];
  let off = 0;
  const emit = (f: EditField, g?: string) => {
    if (f.kind === "group") {
      f.children.forEach((child) => emit(child, f.name.trim()));
      return;
    }
    if (!isPad(f)) {
      const field: Field = { name: f.name.trim(), off, t: toTy(f) };
      if (f.c.trim()) field.c = f.c.trim();
      if (f.e) field.e = f.e;
      if (f.display) field.display = f.display;
      if (f.refs.length) field.refs = f.refs;
      if (g) field.g = g;
      if (f.when.length) {
        field.when = f.when.map((r) => ({ field: r.field, in: r.values, ...(r.not ? { not: true } : {}), t: { k: r.kind } as Ty }));
      }
      out.push(field);
    }
    off += fieldSize(f);
  };
  fields.forEach((f) => emit(f));
  return out;
}

export function toDef(draft: Draft): ListDef {
  const fields = toFields(draft.fields);
  const def: ListDef = { name: draft.name.trim() };
  if (draft.key) def.key = draft.key;
  if (draft.struct.trim()) def.struct = draft.struct.trim();
  if (fields.length) {
    def.size = draftSize(draft.fields);
    def.fields = fields;
  }
  return def;
}

// ---------------------------------------------------------------- rows

export interface Row {
  field: EditField;
  depth: number;
  /** Offset of the field (of the first element, inside arrays of structs). */
  off: number;
  /** Siblings, for moving and removing. */
  parent: EditField[];
  /** The struct or group the field belongs to, if any. */
  owner: EditField | null;
}

export function rows(
  fields: EditField[],
  collapsed: Set<number>,
  depth = 0,
  base = 0,
  owner: EditField | null = null,
  out: Row[] = [],
): Row[] {
  let off = base;
  for (const field of fields) {
    out.push({ field, depth, off, parent: fields, owner });
    if (hasMembers(field.kind) && !collapsed.has(field.uid)) rows(field.children, collapsed, depth + 1, off, field, out);
    off += fieldSize(field);
  }
  return out;
}

// ---------------------------------------------------------------- validation

/** Kinds a rule may switch to: scalars of the same size. */
export const RULE_KINDS: Kind[] = ["i32", "u32", "f32", "i16", "u16", "i8", "u8", "bool", "i64", "u64", "f64"];
const INTEGERS: Kind[] = ["i8", "u8", "bool", "i16", "u16", "i32", "u32", "i64", "u64"];

export const ruleKindsFor = (f: EditField) => RULE_KINDS.filter((k) => kindInfo(k).size === elemSize(f));

/** Fields that can carry type rules: one scalar number. */
export const canHaveRules = (f: EditField) => RULE_KINDS.includes(f.kind) && f.dims.length === 0;

/** Each field's siblings at its level, with groups opened up (they are display only). */
export function levels(fields: EditField[], out = new Map<number, EditField[]>()): Map<number, EditField[]> {
  const flat = fields.flatMap((f) => (f.kind === "group" ? f.children : [f]));
  for (const f of fields) out.set(f.uid, flat);
  for (const f of flat) {
    out.set(f.uid, flat);
    if (f.kind === "struct") levels(f.children, out);
  }
  return out;
}

/** Integer fields next to `f` that a rule can test. */
export const ruleControls = (f: EditField, level: EditField[]) =>
  level.filter((s) => s.uid !== f.uid && INTEGERS.includes(s.kind) && s.dims.length === 0 && !isPad(s));

/** "7, 8, 15-27" → [7, 8, 15, …, 27]; null when it does not parse. */
export function parseValues(text: string): number[] | null {
  const out: number[] = [];
  for (const part of text.split(/[\s,;]+/).filter(Boolean)) {
    const range = /^(-?\d+)\s*(?:-|\.\.)\s*(-?\d+)$/.exec(part);
    if (range) {
      const [a, b] = [Number(range[1]), Number(range[2])];
      if (b < a || b - a > 10000) return null;
      for (let v = a; v <= b; v++) out.push(v);
    } else if (/^-?\d+$/.test(part)) {
      out.push(Number(part));
    } else {
      return null;
    }
  }
  return [...new Set(out)].sort((a, b) => a - b);
}

/** [7, 8, 15, 16, 17] → "7, 8, 15-17". */
export function formatValues(values: number[]): string {
  const sorted = [...new Set(values)].sort((a, b) => a - b);
  const parts: string[] = [];
  for (let i = 0; i < sorted.length; i++) {
    let j = i;
    while (j + 1 < sorted.length && sorted[j + 1] === sorted[j] + 1) j++;
    parts.push(j - i >= 2 ? `${sorted[i]}-${sorted[j]}` : j > i ? `${sorted[i]}, ${sorted[j]}` : String(sorted[i]));
    i = j;
  }
  return parts.join(", ");
}

/** Problems by field uid ("" for the list itself). */
export function validate(draft: Draft): Map<number | "", string> {
  const problems = new Map<number | "", string>();
  const levelOf = levels(draft.fields);
  if (!draft.name.trim()) problems.set("", "The list needs a name.");
  // Group members share their parent's namespace: groups are display only.
  const walk = (fields: EditField[], seen = new Set<string>(), inGroup = false) => {
    for (const f of fields) {
      const name = f.name.trim();
      if (f.kind === "group") {
        if (!name) problems.set(f.uid, "Name the group.");
        else if (inGroup) problems.set(f.uid, "Groups cannot contain groups.");
        else if (!f.children.length) problems.set(f.uid, "A group needs at least one field.");
        walk(f.children, seen, true);
        continue;
      }
      if (!name) problems.set(f.uid, "Name the field.");
      else if (/[.[\]]/.test(name)) problems.set(f.uid, "Names cannot contain . [ or ].");
      else if (seen.has(name)) problems.set(f.uid, `Another field here is named "${name}".`);
      seen.add(name);
      if (kindInfo(f.kind).len && !(f.len >= 1)) problems.set(f.uid, "Length must be at least 1.");
      if (f.dims.some((d) => !(d >= 1))) problems.set(f.uid, "Array sizes must be at least 1.");
      if (f.when.length) {
        const controls = ruleControls(f, levelOf.get(f.uid) ?? []).map((c) => c.name.trim());
        if (!canHaveRules(f)) problems.set(f.uid, "Type rules need a single number field.");
        for (const r of f.when) {
          if (!controls.includes(r.field)) problems.set(f.uid, `A type rule tests "${r.field}", which is not a number field next to this one.`);
          else if (!r.values.length) problems.set(f.uid, "A type rule needs at least one value.");
          else if (kindInfo(r.kind).size !== elemSize(f)) problems.set(f.uid, "A type rule must keep the field's size.");
        }
      }
      if (f.kind === "struct") {
        if (!f.children.length) problems.set(f.uid, "A struct needs at least one member.");
        walk(f.children, new Set());
      }
    }
  };
  walk(draft.fields);
  return problems;
}

/** Renames repeated names within a struct level: a, a → a, a_2. */
export function renameDuplicates(fields: EditField[]): EditField[] {
  const levelNames = (list: EditField[]): string[] =>
    list.flatMap((f) => (f.kind === "group" ? levelNames(f.children) : [f.name.trim()]));
  const used = new Set(levelNames(fields));
  const seen = new Set<string>();
  const rename = (list: EditField[]): EditField[] =>
    list.map((f) => {
      if (f.kind === "group") return { ...f, children: rename(f.children) };
      let name = f.name.trim();
      if (seen.has(name)) {
        let n = 2;
        while (used.has(`${name}_${n}`)) n++;
        name = `${name}_${n}`;
        used.add(name);
      }
      seen.add(name);
      return { ...f, name, children: renameDuplicates(f.children) };
    });
  return rename(fields);
}

/** Uids of every group, e.g. to start with all groups collapsed. */
export function groupUids(fields: EditField[], out: number[] = []): number[] {
  for (const f of fields) {
    if (f.kind === "group") out.push(f.uid);
    groupUids(f.children, out);
  }
  return out;
}

/** The stem shared by numbered fields: "id_addon3" → "id_addon", "sect_mask_2" → "sect_mask". */
const stem = (name: string) => name.trim().replace(/_?\d+$/, "");
const numbered = (name: string) => /\d$/.test(name.trim());
const groupable = (f: EditField) => f.kind !== "group" && !isPad(f);

/**
 * Groups runs of 3+ consecutive numbered fields with the same stem, such as
 * id_addon1…id_addon5, or sect_mask, sect_mask_1…sect_mask_3 (the bare stem
 * may lead). Fields already in a group are left alone; the uids of new groups
 * are added to `created`.
 */
export function autoGroup(fields: EditField[], created: number[] = []): EditField[] {
  const out: EditField[] = [];
  let i = 0;
  while (i < fields.length) {
    const f = fields[i];
    const s = stem(f.name);
    let j = i + 1;
    if (groupable(f) && s) {
      while (j < fields.length && groupable(fields[j]) && numbered(fields[j].name) && stem(fields[j].name) === s) j++;
    }
    if (j - i >= 3) {
      const group = newField({ name: s.replace(/_+$/, ""), kind: "group", children: fields.slice(i, j) });
      created.push(group.uid);
      out.push(group);
      i = j;
    } else {
      out.push(f.kind === "struct" ? { ...f, children: autoGroup(f.children, created) } : f);
      i++;
    }
  }
  return out;
}

export const hasDuplicates = (problems: Map<number | "", string>) =>
  [...problems.values()].some((p) => p.startsWith("Another field here"));

export function parseDims(text: string): number[] | null {
  const t = text.trim();
  if (!t) return [];
  const parts = t.split(/\s*[x×,*]\s*/i).map(Number);
  return parts.every((n) => Number.isInteger(n) && n >= 1) ? parts : null;
}

export const formatDims = (dims: number[]) => dims.join("×");

/** Fields appended to cover the record up to `size`: int32s, then bytes. */
export function fillTo(fields: EditField[], size: number): EditField[] {
  let off = draftSize(fields);
  const added: EditField[] = [];
  while (size - off >= 4) {
    added.push(newField({ name: `unknown_${hex4(off)}`, kind: "i32" }));
    off += 4;
  }
  if (size > off) added.push(newField({ name: `unknown_${hex4(off)}`, kind: "bytes", len: size - off }));
  return [...fields, ...added];
}

// ---------------------------------------------------------------- define at offset

/** Bytes a new field may take over: padding, or a placeholder named unknown_XXXX. */
export const isPlaceholder = (f: EditField) =>
  isPad(f) || (/^unknown_[0-9a-f]+$/i.test(f.name.trim()) && !hasMembers(f.kind) && f.dims.length === 0);

export interface FieldSpec {
  kind: Kind;
  len?: number;
  dims?: number[];
  /** Display role, e.g. "time". */
  display?: string;
}

/**
 * Defines a field at a byte offset of the record, taking over the padding or
 * placeholder fields there (leftover bytes stay padding). The draft is first
 * padded up to `itemSize` so the record's undescribed tail can be used too.
 */
export function defineAt(
  fields: EditField[],
  itemSize: number,
  offset: number,
  spec: FieldSpec,
): { fields: EditField[]; uid: number } | { error: string } {
  const field = newField({
    name: `field_${hex4(offset)}`,
    kind: spec.kind,
    len: spec.len ?? 32,
    dims: spec.dims ?? [],
    display: spec.display ?? "",
  });
  const want = fieldSize(field);
  const size = draftSize(fields);
  const padded = size < itemSize ? [...fields, padField(size, itemSize - size)] : fields;

  const place = (level: EditField[], base: number): EditField[] | string => {
    let off = base;
    for (let i = 0; i < level.length; i++) {
      const f = level[i];
      const fsize = fieldSize(f);
      if (offset < off || offset >= off + fsize) {
        off += fsize;
        continue;
      }
      if (f.kind === "group") {
        const inner = place(f.children, off);
        return typeof inner === "string" ? inner : level.map((x, j) => (j === i ? { ...x, children: inner } : x));
      }
      if (!isPlaceholder(f)) return `Byte ${hex4(offset)} belongs to the field "${f.name}". Only padding and unknown_… fields can be redefined.`;
      // Take over consecutive placeholders until the new field fits.
      let end = off + fsize;
      let j = i + 1;
      while (end < offset + want && j < level.length && isPlaceholder(level[j])) end += fieldSize(level[j++]);
      if (end < offset + want) {
        return `Only ${end - offset} undefined bytes from ${hex4(offset)}; a ${kindInfo(spec.kind).label} of ${want} B does not fit.`;
      }
      const parts = [
        ...(offset > off ? [padField(off, offset - off)] : []),
        field,
        ...(end > offset + want ? [padField(offset + want, end - offset - want)] : []),
      ];
      return [...level.slice(0, i), ...parts, ...level.slice(j)];
    }
    return `Offset ${hex4(offset)} is outside the record.`;
  };

  const placed = place(padded, 0);
  return typeof placed === "string" ? { error: placed } : { fields: placed, uid: field.uid };
}
