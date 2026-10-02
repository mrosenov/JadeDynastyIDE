#!/usr/bin/env node
// Applies the conditional field types in tools/type-rules.json to every
// layout in src-tauri/formats/layouts. Run it after regenerating layouts.
//
//   node tools/apply-type-rules.mjs

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { readLayouts, writeLayout } from "./lib/layouts.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const { rules } = JSON.parse(fs.readFileSync(path.join(here, "type-rules.json"), "utf8"));

for (const layout of readLayouts()) {
  let applied = 0;
  for (const def of layout.lists) {
    if (!def?.fields) continue;
    for (const rule of rules) {
      const matches = rule.struct ? def.struct === rule.struct : def.name === rule.list;
      if (!matches) continue;
      const field = def.fields.find((f) => f.name === rule.field);
      const size = field && { i32: 4, u32: 4, f32: 4 }[field.t.k];
      const controlsOk = rule.when.every((w) => def.fields.some((f) => f.name === w.field));
      if (!field || !size || !controlsOk) {
        console.warn(`  ${layout.id}: ${def.struct ?? def.name}.${rule.field} not found or not a 4-byte scalar, skipped`);
        continue;
      }
      field.when = rule.when;
      applied++;
    }
  }
  if (applied) {
    writeLayout(layout);
    console.log(`${layout.id}: ${applied} rule(s) applied`);
  }
}
