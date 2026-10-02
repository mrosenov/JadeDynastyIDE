#!/usr/bin/env node
// Converts the JadeEditorPython format files (formats/elements) into a jdide
// layout for elements.data v112, the version the original Jade Editor targets.
//
//   node tools/import-jade-editor-profile.mjs <JadeEditorPython/formats/elements> <layouts/v112.json>

import fs from "node:fs";
import path from "node:path";

const [formatsDir, outPath] = process.argv.slice(2);
if (!formatsDir || !outPath) {
  console.error("usage: import-jade-editor-profile.mjs <formats/elements dir> <out.json>");
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
    if (f.enum) field.e = f.enum;
    if (f.flags) flagEnums.add(f.enum);
    return field;
  });
  Object.assign(enumSets, schema.enums ?? {});
  schemas.set(schema.list_type, { size: schema.item_size, fields });
}

const enums = Object.fromEntries(
  Object.entries(enumSets).map(([key, items]) => [key, { label: key, flags: flagEnums.has(key), items }]),
);

const layout = {
  id: path.basename(outPath, ".json"),
  version: VERSION,
  source: "Jade Editor list names and schemas (JadeEditorPython)",
  markers: [
    { before: 23, kind: "exporter" },
    { before: 56, kind: "tag" },
  ],
  enums,
  lists: names.map((name) => ({ name, ...(schemas.get(name) ?? {}) })),
};

fs.mkdirSync(path.dirname(outPath), { recursive: true });
fs.writeFileSync(outPath, JSON.stringify(layout));
console.log(`wrote ${outPath}: v${VERSION}, ${layout.lists.length} lists, ${schemas.size} typed`);
