// Named value sets: formats/enums/<key>.json and formats/masks/<key>.json.
//
//   enum: { key, label, values: [{ value, label, description? }] }
//   mask: { key, label, flags:  [{ bit,   label, description? }] }   (bit = 0…63)

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const FORMATS_DIR = path.join(path.dirname(fileURLToPath(import.meta.url)), "../../src-tauri/formats");

const clean = (items) =>
  items.map(({ description, ...rest }) => (description ? { ...rest, description } : rest));

/** Writes one set as formats/<enums|masks>/<key>.json. */
export function writeSet(set, root = FORMATS_DIR) {
  const kind = set.flags ? "masks" : "enums";
  const dir = path.join(root, kind);
  fs.mkdirSync(dir, { recursive: true });
  const body = set.flags
    ? { key: set.key, label: set.label ?? set.key, flags: clean([...set.flags].sort((a, b) => a.bit - b.bit)) }
    : { key: set.key, label: set.label ?? set.key, values: clean([...set.values].sort((a, b) => a.value - b.value)) };
  fs.writeFileSync(path.join(dir, `${set.key}.json`), JSON.stringify(body, null, 2) + "\n");
}

/** Removes the set files whose key starts with `prefix` (before rewriting them). */
export function clearSets(prefix, root = FORMATS_DIR) {
  for (const kind of ["enums", "masks"]) {
    const dir = path.join(root, kind);
    if (!fs.existsSync(dir)) continue;
    for (const f of fs.readdirSync(dir)) if (f.endsWith(".json") && f.startsWith(prefix)) fs.rmSync(path.join(dir, f));
  }
}

/** "TradeBehavior" → "trade_behavior". */
export const snake = (name) =>
  name
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .replace(/[^A-Za-z0-9]+/g, "_")
    .toLowerCase()
    .replace(/^_+|_+$/g, "");
