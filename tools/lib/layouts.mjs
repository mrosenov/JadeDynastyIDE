// Layout folders: formats/layouts/<id>/layout.json + list_<n>.json.
//
// layout.json holds everything but the list definitions:
//   { id, version, source, markers, listCount, enums?, listCountUnverified? }
// list_<n>.json holds the definition of list slot n (absent = not defined).

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const LAYOUTS_DIR = path.join(path.dirname(fileURLToPath(import.meta.url)), "../../src-tauri/formats/layouts");

const LIST_FILE = /^list_(\d+)\.json$/;
const readJson = (p) => JSON.parse(fs.readFileSync(p, "utf8"));

/** Reads a layout folder into { id, version, source, markers, lists: [def | null], … }. */
export function readLayout(dir) {
  const meta = readJson(path.join(dir, "layout.json"));
  const { listCount = 0, ...rest } = meta;
  const lists = new Array(listCount).fill(null);
  for (const file of fs.readdirSync(dir)) {
    const m = LIST_FILE.exec(file);
    if (m) lists[Number(m[1])] = readJson(path.join(dir, file));
  }
  for (let i = 0; i < lists.length; i++) lists[i] ??= null;
  return { ...rest, lists };
}

/** Every layout under `root`, sorted by version then id. */
export function readLayouts(root = LAYOUTS_DIR) {
  if (!fs.existsSync(root)) return [];
  return fs
    .readdirSync(root, { withFileTypes: true })
    .filter((e) => e.isDirectory() && fs.existsSync(path.join(root, e.name, "layout.json")))
    .map((e) => readLayout(path.join(root, e.name)))
    .sort((a, b) => a.version - b.version || a.id.localeCompare(b.id));
}

/** Writes a layout as a folder under `root`, replacing any previous list files. */
export function writeLayout(layout, root = LAYOUTS_DIR) {
  const dir = path.join(root, layout.id);
  fs.mkdirSync(dir, { recursive: true });
  for (const file of fs.readdirSync(dir)) {
    if (LIST_FILE.test(file)) fs.rmSync(path.join(dir, file));
  }
  const { lists, ...meta } = layout;
  fs.writeFileSync(path.join(dir, "layout.json"), JSON.stringify({ ...meta, listCount: lists.length }, null, 2) + "\n");
  lists.forEach((def, i) => {
    if (def) fs.writeFileSync(path.join(dir, `list_${i}.json`), JSON.stringify(def, null, 2) + "\n");
  });
  return dir;
}
