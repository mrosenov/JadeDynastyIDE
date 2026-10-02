// Minimal elements.data reader for the build tools (validation only).
// Mirrors src-tauri/src/elements/reader.rs: lists, markers before given list
// slots, and the talk_proc block running to the end of the file.

import fs from "node:fs";

const MAX_ITEM_SIZE = 1 << 20;

/** Bytes a marker occupies at `p`. */
function markerSize(d, p, kind) {
  switch (kind) {
    case "checksum":
      return 8;
    case "exporter":
      return 12 + d.readUInt32LE(p + 4);
    case "tag":
      return 8 + d.readUInt32LE(p + 4);
    default:
      throw new Error(`unknown marker kind ${kind}`);
  }
}

/** True when the bytes from `p` to EOF are exactly the talk_proc block. */
export function isTalkBlock(d, p) {
  const n = d.length;
  if (p + 4 > n) return false;
  const count = d.readUInt32LE(p);
  p += 4;
  if (count > 1000000) return false;
  for (let i = 0; i < count; i++) {
    if (p + 136 > n) return false;
    const windows = d.readInt32LE(p + 132);
    p += 136;
    if (windows < 0 || windows > 100000) return false;
    for (let w = 0; w < windows; w++) {
      if (p + 12 > n) return false;
      const textLen = d.readInt32LE(p + 8);
      p += 12;
      if (textLen < 0 || p + textLen * 2 + 4 > n) return false;
      p += textLen * 2;
      const options = d.readInt32LE(p);
      p += 4;
      if (options < 0 || p + options * 136 > n) return false;
      p += options * 136;
    }
  }
  return p === n;
}

/**
 * Reads the list record sizes using a marker table
 * ([{ before, kind }]). Returns null when the file does not fit it.
 */
export function readListSizes(file, markers) {
  const d = typeof file === "string" ? fs.readFileSync(file) : file;
  const byIndex = new Map(markers.map((m) => [m.before, m.kind]));
  const lastMarker = Math.max(-1, ...markers.map((m) => m.before));
  const sizes = [];
  let p = 8;
  for (;;) {
    const kind = byIndex.get(sizes.length);
    if (kind) {
      if (p + 8 > d.length) return null;
      p += markerSize(d, p, kind);
    }
    if (sizes.length >= lastMarker && isTalkBlock(d, p)) break;
    if (p + 8 > d.length) return null;
    const size = d.readUInt32LE(p);
    const count = d.readUInt32LE(p + 4);
    if (size === 0 || size > MAX_ITEM_SIZE || p + 8 + size * count > d.length) return null;
    sizes.push(size);
    p += 8 + size * count;
  }
  return { version: d.readUInt32LE(0) & 0xffff, sizes };
}

/** Checks a layout's list sizes against a file; returns a list of problems. */
export function validateLayout(layout, file) {
  const actual = readListSizes(file, layout.markers);
  if (!actual) return [`file does not fit the marker table`];
  const problems = [];
  if (actual.version !== layout.version) problems.push(`file is v${actual.version}, layout is v${layout.version}`);
  if (actual.sizes.length !== layout.lists.length) {
    problems.push(`list count: layout ${layout.lists.length}, file ${actual.sizes.length}`);
  }
  layout.lists.forEach((l, i) => {
    if (l && l.size !== undefined && actual.sizes[i] !== l.size) {
      problems.push(`#${i} ${l.struct ?? l.name}: layout ${l.size} B, file ${actual.sizes[i]} B`);
    }
  });
  return problems;
}
