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
import { clearSets } from "./lib/sets.mjs";

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

// The Jade Editor's own sets are older, partly "Unknown…" copies of shared
// ones; its fields use the shared sets instead.
const SHARED_SET = {
  TradeBehavior: "trade_behavior",
  GearSlotType: "equip_mask",
  Factions: "sect_mask",
  Gender: "gender",
  AttribType: "addon_type",
  YesNo: "bool",
};

const schemas = new Map();
for (const file of fs.readdirSync(path.join(formatsDir, "schemas"))) {
  const schema = readJson("schemas", file);
  if (schema.version !== undefined && schema.version !== VERSION) continue;
  const fields = schema.fields.map((f) => {
    const toType = TYPES[f.type];
    if (!toType) throw new Error(`${file}: unsupported field type ${f.type}`);
    const field = { name: f.name, off: f.offset, t: toType(f) };
    if (f.enum) {
      if (SHARED_SET[f.enum]) field.e = SHARED_SET[f.enum];
      else console.warn(`  ${file}: ${f.name} uses ${f.enum}, which has no shared set; left without one`);
    }
    return field;
  });
  schemas.set(schema.list_type, { size: schema.item_size, fields });
}

// Earlier imports wrote v112_* sets.
clearSets(`v${VERSION}_`);

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
