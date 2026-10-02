#!/usr/bin/env node
// Reports how well each layout of a file's version describes it.
//
//   node tools/check-layouts.mjs <elements.data> [...]

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { readListSizes } from "./lib/elements-file.mjs";

const files = process.argv.slice(2);
if (!files.length) {
  console.error("usage: check-layouts.mjs <elements.data> [...]");
  process.exit(1);
}

const dir = path.join(path.dirname(fileURLToPath(import.meta.url)), "../src-tauri/formats/layouts");
const layouts = fs.readdirSync(dir).map((f) => JSON.parse(fs.readFileSync(path.join(dir, f), "utf8")));

for (const file of files) {
  const data = fs.readFileSync(file);
  const version = data.readUInt32LE(0) & 0xffff;
  const candidates = layouts.filter((l) => l.version === version);
  console.log(`${file}  (v${version}, ${(data.length / 1048576).toFixed(1)} MB)`);
  if (!candidates.length) console.log("  no layout for this version");
  for (const layout of candidates) {
    const actual = readListSizes(data, layout.markers);
    if (!actual) {
      console.log(`  ${layout.id}: does not fit the marker table`);
      continue;
    }
    const tally = { exact: 0, partial: 0, wrong: 0, named: 0, undefined: 0 };
    const wrong = [];
    actual.sizes.forEach((size, i) => {
      const def = layout.lists[i];
      if (!def) tally.undefined++;
      else if (def.size === undefined) tally.named++;
      else if (def.size === size) tally.exact++;
      else if (def.size < size) tally.partial++;
      else {
        tally.wrong++;
        wrong.push(`#${i} ${def.struct ?? def.name} ${def.size}>${size}`);
      }
    });
    const count = actual.sizes.length === layout.lists.length ? `${actual.sizes.length} lists` : `${actual.sizes.length} lists (layout has ${layout.lists.length})`;
    console.log(
      `  ${layout.id.padEnd(12)} ${count}: ${tally.exact} exact, ${tally.partial} partial, ${tally.wrong} wrong, ${tally.named} name only, ${tally.undefined} undefined`,
    );
    if (wrong.length) console.log(`    wrong: ${wrong.slice(0, 10).join(", ")}${wrong.length > 10 ? ", …" : ""}`);
  }
}
