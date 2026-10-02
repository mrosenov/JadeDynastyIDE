#!/usr/bin/env node
// Converts the JadeEditorPython format files (formats/elements) into a jdide
// layout for elements.data v112, the version the original Jade Editor targets.
//
//   node tools/import-jade-editor-profile.mjs <JadeEditorPython/formats/elements> [layout-id]
//
// Writes src-tauri/formats/layouts/<layout-id>/ (default id: v112).

import fs from "node:fs";
import path from "node:path";
import { writeLayout } from "./lib/layouts.mjs";
import { clearSets, snake, writeSet } from "./lib/sets.mjs";

const [formatsDir, layoutId = "v112"] = process.argv.slice(2);
if (!formatsDir) {
  console.error("usage: import-jade-editor-profile.mjs <formats/elements dir> [layout-id]");
  process.exit(1);
}

const VERSION = 112;
const readJson = (...p) => JSON.parse(fs.readFileSync(path.join(formatsDir, ...p), "utf8"));

const listCount = readJson("versions.json")[String(VERSION)].list_count;
// Index 0 is a "DefaultList" placeholder; list N in the file is names[N].
const names = readJson("list_types.json").slice(1, listCount + 1);

const TYPES = {
  int32: () => ({ k: "i32" }),
  float32: () => ({ k: "f32" }),
  dynamic_number: () => ({ k: "i32" }),
  utf16le_fixed: (f) => ({ k: "wstr", n: f.chars }),
};

const enumSets = {};
const flagEnums = new Set();
const schemas = new Map();
for (const file of fs.readdirSync(path.join(formatsDir, "schemas"))) {
  const schema = readJson("schemas", file);
  if (schema.version !== undefined && schema.version !== VERSION) continue;
  const fields = schema.fields.map((f) => {
    const toType = TYPES[f.type];
    if (!toType) throw new Error(`${file}: unsupported field type ${f.type}`);
    const field = { name: f.name, off: f.offset, t: toType(f) };
    // Sets are shared by all layouts: prefix them, e.g. TradeBehavior → v112_trade_behavior.
    if (f.enum) field.e = `v${VERSION}_${snake(f.enum)}`;
    if (f.flags) flagEnums.add(f.enum);
    return field;
  });
  Object.assign(enumSets, schema.enums ?? {});
  schemas.set(schema.list_type, { size: schema.item_size, fields });
}

// The Jade Editor's sets become shared enum and mask files.
clearSets(`v${VERSION}_`);
for (const [name, items] of Object.entries(enumSets)) {
  const key = `v${VERSION}_${snake(name)}`;
  const label = `${name.replace(/([a-z])([A-Z])/g, "$1 $2").replace(/_/g, " ")} (v${VERSION})`;
  const entries = Object.entries(items).map(([value, text]) => [Number(value), text.replace(/_/g, " ")]);
  if (flagEnums.has(name)) {
    writeSet({ key, label, flags: entries.map(([v, l]) => ({ bit: Math.log2(v), label: l })) });
  } else {
    writeSet({ key, label, values: entries.map(([v, l]) => ({ value: v, label: l })) });
  }
}

const layout = {
  id: layoutId,
  version: VERSION,
  source: "Jade Editor list names and schemas (JadeEditorPython)",
  markers: [
    { before: 23, kind: "exporter" },
    { before: 56, kind: "tag" },
  ],
  lists: names.map((name) => ({ name, ...(schemas.get(name) ?? {}) })),
};

const outDir = writeLayout(layout);
console.log(`wrote ${outDir}: v${VERSION}, ${layout.lists.length} lists, ${schemas.size} typed`);
