#!/usr/bin/env node
// Imports the mask label tables of the official element data editor
// (ZElementData). Tables an existing mask already covers add their labels to
// its flag descriptions; the others become built-in masks named zx_<type>.
// Run it after import-laravel-structures.mjs, which rewrites those masks.
//
//   node tools/import-official-sets.mjs "<ZX-Client-Src>/ZElement/ZElementData"
//
// The editor hard-codes them in ElementDataDoc.cpp (one LPCTSTR array per
// field type, bound with `m_pMaskNames = _array`) and ProcTypeMaskDlg.cpp
// (proc_type). Bit i is the i-th label.

import fs from "node:fs";
import path from "node:path";
import { FORMATS_DIR, clearSets, writeSet } from "./lib/sets.mjs";

const [dir] = process.argv.slice(2);
if (!dir) {
  console.error('usage: import-official-sets.mjs "<ZX-Client-Src>/ZElement/ZElementData"');
  process.exit(1);
}

const gbk = new TextDecoder("gbk");
const read = (f) => gbk.decode(fs.readFileSync(path.join(dir, f)));

/** Every `name[] = { "…", _T("…"), … };` array in a source file. */
function arrays(text) {
  const out = new Map();
  for (const m of text.matchAll(/(\w+)\s*\[\s*\w*\s*\]\s*=\s*\{/g)) {
    const end = text.indexOf("};", m.index);
    const body = text.slice(m.index + m[0].length, end);
    out.set(m[1], [...body.matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((s) => s[1]));
  }
  return out;
}

const doc = read("ElementDataDoc.cpp");
const tables = arrays(doc);
const bindings = new Map();
// `if(stricmp(type,"x")==0) … m_pMaskNames = _array;` (comments may sit in between)
for (const m of doc.matchAll(/stricmp\(\s*type\s*,\s*"(\w+)"\s*\)\s*==\s*0/g)) {
  const near = doc.slice(m.index, m.index + 700);
  const next = near.slice(10).search(/stricmp\(\s*type/);
  const block = next < 0 ? near : near.slice(0, next + 10);
  const bound = /m_pMaskNames\s*=\s*(\w+)/.exec(block);
  if (bound && tables.has(bound[1])) bindings.set(m[1], tables.get(bound[1]));
}
const proc = [...arrays(read("ProcTypeMaskDlg.cpp")).values()].find((a) => a.length > 8);
if (proc) bindings.set("proc_type", proc);

const title = (type) =>
  type
    .split("_")
    .map((w) => w[0].toUpperCase() + w.slice(1))
    .join(" ");

// Tables that an existing mask already covers bit for bit (checked by hand):
// their labels become the descriptions of that mask's flags instead of a
// second set of the same thing.
const SAME_AS = {
  equip_mask: "equip_mask",
  sect_type: "sect_mask",
  character_combo_id: "character_combo_id",
  character_combo_id2: "character_combo_id2",
  combined_services: "combined_services",
  combined_services2: "combined_services2",
  combined_services3: "combined_services3",
  immune_type: "immune_type",
  forbid_food: "forbid_food",
  god_devil_mask: "god_devil_mask",
  nation_position_mask: "nation_position_mask",
  proc_type: "trade_behavior",
};

// Unused class slots read "c63:"; keep the slot name.
const cleanLabel = (raw, bit) => raw.trim().replace(/:$/, "") || `bit ${bit}`;
// A label that names nothing: "c15", "bit 3".
const placeholder = (label) => /^(c\d+|bit \d+)$/.test(label);

clearSets("zx_");
const written = [];
const merged = [];
for (const [type, labels] of bindings) {
  const flags = labels.map((raw, bit) => ({ bit, label: cleanLabel(raw, bit) }));
  const target = SAME_AS[type];
  if (!target) {
    writeSet({ key: `zx_${type}`, label: `${title(type)} (official)`, flags });
    written.push(type);
    continue;
  }
  const file = path.join(FORMATS_DIR, "masks", `${target}.json`);
  const set = JSON.parse(fs.readFileSync(file, "utf8"));
  for (const { bit, label } of flags) {
    if (placeholder(label)) continue;
    const flag = set.flags.find((f) => f.bit === bit);
    if (!flag) set.flags.push({ bit, label, description: `Official: ${label}` });
    else if (!flag.description) flag.description = `Official: ${label}`;
    else if (!flag.description.includes(label)) flag.description += ` (official: ${label})`;
  }
  writeSet(set);
  merged.push(`${type}→${target}`);
}
console.log(`wrote ${written.length} official masks: ${written.join(" ")}`);
console.log(`merged ${merged.length} into existing masks: ${merged.join(" ")}`);
