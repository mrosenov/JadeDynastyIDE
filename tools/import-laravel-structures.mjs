#!/usr/bin/env node
// Imports the curated list schemas of the Laravel jdide project
// (resources/structures/elements) into jdide layouts.
//
//   node tools/import-laravel-structures.mjs <structures-dir> <formats-dir> [elements.data ...]
//
// - <structures-dir>/vNNN/list_N.json  → <formats-dir>/layouts/vNNN/
// - <structures-dir>/masks.json        → <formats-dir>/enums.json
// - lists whose struct matches a source-generated layout already in
//   <formats-dir>/layouts (same struct name and size) keep that richer
//   definition, with the Laravel display/mask/ref hints merged into it.
// - refs become struct names (e.g. "EQUIPMENT_ADDON") rather than list
//   indexes, so a definition shared between versions stays correct.
// - the sample files fix each version's list count and validate the result.

import fs from "node:fs";
import path from "node:path";
import { readListSizes, validateLayout } from "./lib/elements-file.mjs";
import { readLayouts, writeLayout } from "./lib/layouts.mjs";

const [structuresDir, formatsDir, ...samples] = process.argv.slice(2);
if (!structuresDir || !formatsDir) {
  console.error("usage: import-laravel-structures.mjs <structures-dir> <formats-dir> [elements.data ...]");
  process.exit(1);
}

const layoutsDir = path.join(formatsDir, "layouts");
const readJson = (p) => JSON.parse(fs.readFileSync(p, "utf8"));
const humanize = (s) =>
  s
    .toLowerCase()
    .split("_")
    .filter(Boolean)
    .map((w) => w[0].toUpperCase() + w.slice(1))
    .join(" ");

// The Laravel reader's marker table (ElementsFile::BASE_MARKERS plus
// ElementsVersion::extraMarkers).
const BASE_MARKERS = [
  { before: 13, kind: "checksum" },
  { before: 23, kind: "exporter" },
  { before: 36, kind: "checksum" },
  { before: 56, kind: "tag" },
  { before: 62, kind: "checksum" },
  { before: 103, kind: "checksum" },
];
const EXTRA_MARKERS = { 165: [{ before: 296, kind: "checksum" }], 176: [{ before: 296, kind: "checksum" }] };

// ---------------------------------------------------------------- enums

function importEnums() {
  const sets = readJson(path.join(structuresDir, "masks.json"));
  const enums = {};
  for (const set of sets) {
    const flags = (set.type ?? "flags") === "flags";
    const items = {};
    const descriptions = {};
    for (const item of flags ? set.flags : set.values) {
      const value = flags ? (1n << BigInt(item.bit)).toString() : String(item.value);
      items[value] = item.label;
      if (item.description) descriptions[value] = item.description;
    }
    enums[set.key] = { label: set.label, flags, items, descriptions };
  }
  return enums;
}

// ---------------------------------------------------------------- fields

function convertType(token) {
  const [kind, width] = token.split(":");
  const n = Number(width);
  switch (kind) {
    case "int32":
      return { t: { k: "i32" }, size: 4 };
    case "int64":
      return { t: { k: "i64" }, size: 8 };
    case "float":
      return { t: { k: "f32" }, size: 4 };
    case "double":
      return { t: { k: "f64" }, size: 8 };
    case "wstring":
      return { t: { k: "wstr", n: n / 2 }, size: n };
    case "string":
      return { t: { k: "str", n }, size: n };
    case "byte":
      if (n === 1) return { t: { k: "u8" }, size: 1 };
      if (n === 2) return { t: { k: "u16" }, size: 2 };
      return { t: { k: "bytes", n }, size: n };
    default:
      throw new Error(`unknown field type ${token}`);
  }
}

/** Display/mask/enum/ref hints of a Laravel field, in jdide field keys. */
function hintsOf(field) {
  const hints = {};
  if (field.display === "mask" && field.mask) hints.e = field.mask;
  else if (field.display === "enum" && field.enum) hints.e = field.enum;
  else if (field.display) hints.display = field.display;
  if (field.refs?.length) hints.refs = field.refs.map(Number);
  return hints;
}

function convertList(schema) {
  let off = 0;
  const fields = schema.fields.map((f) => {
    const { t, size } = convertType(f.type);
    const field = { name: f.name, off, t, ...hintsOf(f) };
    off += size;
    return field;
  });
  return { name: humanize(schema.name || "unnamed"), struct: schema.name || undefined, size: off, fields, flat: true };
}

/** Leaf fields of a nested definition with absolute offsets. */
function* leaves(fields, base = 0) {
  for (const f of fields) {
    yield* leafOf(f, f.t, base + f.off);
  }
}
function* leafOf(field, t, off) {
  if (t.k === "struct") yield* leaves(t.fields, off);
  else if (t.k === "array") for (let i = 0; i < t.n; i++) yield* leafOf(field, t.t, off + i * t.stride);
  else yield { field, off, t };
}

const scalarSize = (t) => ({ i8: 1, u8: 1, bool: 1, i16: 2, u16: 2, i32: 4, u32: 4, f32: 4, f64: 8, i64: 8, u64: 8 })[t.k];

/**
 * Copies Laravel hints onto a source-generated definition's fields, matched by
 * offset. Hints on array elements apply to the whole array member.
 */
function mergeHints(target, flat) {
  const byOffset = new Map();
  for (const leaf of leaves(target.fields)) if (!byOffset.has(leaf.off)) byOffset.set(leaf.off, leaf);
  let merged = 0;
  for (const f of flat.fields) {
    const hints = Object.fromEntries(Object.entries(f).filter(([k]) => ["e", "display", "refs"].includes(k)));
    if (!Object.keys(hints).length) continue;
    const leaf = byOffset.get(f.off);
    if (!leaf || scalarSize(leaf.t) === undefined) continue;
    for (const [k, v] of Object.entries(hints)) if (leaf.field[k] === undefined) leaf.field[k] = v;
    merged++;
  }
  return merged;
}

// ---------------------------------------------------------------- main

const enums = importEnums();
fs.mkdirSync(layoutsDir, { recursive: true });
fs.writeFileSync(path.join(formatsDir, "enums.json"), JSON.stringify(enums, null, 1));
console.log(`wrote enums.json: ${Object.keys(enums).length} sets`);

// Source-generated layouts, indexed by struct name, to prefer and enrich.
const sourceLayouts = readLayouts(layoutsDir)
  .map((layout) => ({ layout }))
  .filter(({ layout }) => layout.source?.startsWith("Server template sources"));
// Keyed by struct and size: variants of one version may define a struct
// with different sizes.
const sourceByStruct = new Map();
for (const { layout } of sourceLayouts) {
  for (const list of layout.lists) {
    if (!list?.struct) continue;
    const key = `${list.struct}/${list.size}`;
    sourceByStruct.set(key, [...(sourceByStruct.get(key) ?? []), list]);
  }
}

const sampleVersions = samples.map((file) => ({ file, version: fs.readFileSync(file).readUInt32LE(0) & 0xffff }));

const versionDirs = fs
  .readdirSync(structuresDir)
  .filter((d) => /^v\d+$/.test(d))
  .sort((a, b) => Number(a.slice(1)) - Number(b.slice(1)));

for (const dir of versionDirs) {
  const version = Number(dir.slice(1));
  const flatLists = [];
  for (const f of fs.readdirSync(path.join(structuresDir, dir))) {
    const m = /^list_(\d+)\.json$/.exec(f);
    if (m) flatLists[Number(m[1])] = convertList(readJson(path.join(structuresDir, dir, f)));
  }

  // Lists matching a source layout's struct: merge hints into the source
  // layout itself, and reuse its nested definition here.
  let reused = 0;
  // Laravel refs are list indexes of this version; name them by struct.
  for (const flat of flatLists) {
    for (const field of flat?.fields ?? []) {
      if (!field.refs) continue;
      field.refs = [...new Set(field.refs.map((r) => flatLists[r]?.struct).filter((s) => s && !/^UNKNOWN/i.test(s)))];
      if (!field.refs.length) delete field.refs;
    }
  }

  const lists = Array.from(flatLists, (flat) => {
    if (!flat) return null;
    const sources = flat.struct ? sourceByStruct.get(`${flat.struct}/${flat.size}`) : undefined;
    if (sources) {
      sources.forEach((source) => mergeHints(source, flat));
      reused++;
      return sources[0];
    }
    delete flat.flat;
    return flat;
  });

  // A version generated from source keeps its own layout; only hints merge in.
  if (sourceLayouts.some(({ layout }) => layout.id === dir)) {
    console.log(`${dir}: source layout exists, merged hints for ${reused} lists`);
    continue;
  }

  const markers = [...BASE_MARKERS, ...(EXTRA_MARKERS[version] ?? [])];
  const sample = sampleVersions.find((s) => s.version === version);
  if (sample) {
    const actual = readListSizes(sample.file, markers);
    if (!actual) throw new Error(`${sample.file} does not fit the v${version} marker table`);
    lists.length = Math.max(lists.length, actual.sizes.length);
  }
  for (let i = 0; i < lists.length; i++) lists[i] ??= null;

  const layout = {
    id: dir,
    version,
    source: `Laravel jdide structures (${dir}): ${flatLists.filter(Boolean).length} curated lists`,
    markers,
    lists,
  };
  if (!sample) layout.listCountUnverified = true;
  writeLayout(layout, layoutsDir);

  const defined = lists.filter(Boolean).length;
  let report = `wrote ${dir}/: ${lists.length} lists, ${defined} defined (${reused} from source)`;
  if (sample) {
    const problems = validateLayout(layout, sample.file);
    report += problems.length ? `, ${problems.length} mismatch(es) vs ${sample.file}` : `, all sizes match ${sample.file}`;
    problems.forEach((p) => (report += `\n    ${p}`));
  } else {
    report += ", no sample file to validate against";
  }
  console.log(report);
}

// Write back the source layouts with merged hints.
for (const { layout } of sourceLayouts) writeLayout(layout, layoutsDir);
console.log(`merged hints into ${sourceLayouts.map(({ layout }) => layout.id).join(", ")}`);
