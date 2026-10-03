#!/usr/bin/env node
// Applies tools/type-rules.json to every layout in src-tauri/formats/layouts:
// the conditional field types (`rules`), the name-based enum/mask
// assignments (`sets`), display roles (`roles`) and ref fixes (`refs`). Run it after
// regenerating layouts.
//
//   node tools/apply-type-rules.mjs

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { readLayouts, writeLayout } from "./lib/layouts.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const { rules, sets = [], roles = [], refs = [] } = JSON.parse(fs.readFileSync(path.join(here, "type-rules.json"), "utf8"));

const INTEGER = new Set(["i8", "u8", "i16", "u16", "i32", "u32", "i64", "u64"]);
const setRules = sets.map((r) => ({ ...r, re: new RegExp(r.name, "i") }));
const roleRules = roles.map((r) => ({ ...r, re: new RegExp(r.name, "i") }));
const refRules = refs.map((r) => ({ ...r, re: new RegExp(r.name, "i") }));

/** Every field of a list, members of structs and struct arrays included. */
function* allFields(fields) {
  for (const f of fields ?? []) {
    yield f;
    yield* allFields(f.t.fields);
    yield* allFields(f.t.t?.fields);
  }
}

const layouts = readLayouts();

// Set use per rule: counts of `e` among the fields it matches, overall and per layout.
const usage = new Map(setRules.map((r) => [r, { all: new Map(), byLayout: new Map() }]));
for (const layout of layouts) {
  for (const def of layout.lists) {
    for (const f of allFields(def?.fields)) {
      const rule = f.e && setRules.find((r) => r.re.test(f.name));
      if (!rule) continue;
      const u = usage.get(rule);
      const here = u.byLayout.get(layout.id) ?? new Map();
      u.byLayout.set(layout.id, here);
      for (const m of [u.all, here]) m.set(f.e, (m.get(f.e) ?? 0) + 1);
    }
  }
}
const mostUsed = (counts) => counts && [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))[0]?.[0];

for (const layout of layouts) {
  let applied = 0;
  let named = 0;
  let roled = 0;
  let refFixes = 0;
  for (const def of layout.lists) {
    for (const f of allFields(def?.fields)) {
      const rule = refRules.find((r) => (!r.struct || r.struct === def.struct) && r.re.test(f.name));
      if (!rule || JSON.stringify(f.refs ?? []) === JSON.stringify(rule.refs)) continue;
      if (rule.refs.length) f.refs = rule.refs;
      else delete f.refs;
      refFixes++;
    }
  }
  for (const def of layout.lists) {
    for (const f of allFields(def?.fields)) {
      if (f.display || !INTEGER.has(f.t.k)) continue;
      const rule = roleRules.find((r) => (!r.struct || r.struct === def.struct) && r.re.test(f.name));
      if (!rule) continue;
      f.display = rule.display;
      roled++;
    }
  }
  for (const def of layout.lists) {
    for (const f of allFields(def?.fields)) {
      if (f.e || !INTEGER.has(f.t.k)) continue;
      const rule = setRules.find((r) => r.re.test(f.name));
      if (!rule) continue;
      const u = usage.get(rule);
      f.e = mostUsed(u.byLayout.get(layout.id)) ?? mostUsed(u.all) ?? rule.set;
      named++;
    }
  }
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
  if (applied || named || roled || refFixes) {
    writeLayout(layout);
    console.log(`${layout.id}: ${applied} type rule(s), ${named} enum/mask assignment(s), ${roled} display role(s), ${refFixes} ref fix(es)`);
  }
}
