#!/usr/bin/env node
// Generates an elements.data format profile from the server's template sources.
//
//   node tools/gen-elements-schema.mjs <template-dir> <layout-id> [elements.data to validate]
//
// The layout is written to src-tauri/formats/layouts/<layout-id>/.
//
// <template-dir> must contain exptypes.h, elementdataman.h and elementdataman.cpp
// (e.g. zx_source/zgame/gs/template). The list order comes from
// elementdataman::load_data, the record layouts from the structs in exptypes.h,
// laid out with the same #pragma pack(4) rules the game uses.

import fs from "node:fs";
import path from "node:path";
import { validateLayout } from "./lib/elements-file.mjs";
import { writeLayout } from "./lib/layouts.mjs";

const [templateDir, layoutId, validatePath] = process.argv.slice(2);
if (!templateDir || !layoutId) {
  console.error("usage: gen-elements-schema.mjs <template-dir> <layout-id> [elements.data]");
  process.exit(1);
}

const gbk = new TextDecoder("gbk");
const readSource = (name) => gbk.decode(fs.readFileSync(path.join(templateDir, name)));

const PACK = 4;
const PRIMITIVES = {
  "int": { k: "i32", size: 4 },
  "unsigned int": { k: "u32", size: 4 },
  "signed int": { k: "i32", size: 4 },
  "long": { k: "i32", size: 4 },
  "unsigned long": { k: "u32", size: 4 },
  "short": { k: "i16", size: 2 },
  "unsigned short": { k: "u16", size: 2 },
  "namechar": { k: "u16", size: 2 },
  "char": { k: "i8", size: 1 },
  "signed char": { k: "i8", size: 1 },
  "unsigned char": { k: "u8", size: 1 },
  "bool": { k: "bool", size: 1 },
  "float": { k: "f32", size: 4 },
  "double": { k: "f64", size: 8 },
  "UInt64": { k: "u64", size: 8 },
  "__int64": { k: "i64", size: 8 },
  "unsigned __int64": { k: "u64", size: 8 },
  "long long": { k: "i64", size: 8 },
  "unsigned long long": { k: "u64", size: 8 },
};

// ---------------------------------------------------------------- tokenizer

function tokenize(src) {
  const tokens = [];
  const lines = src.split(/\r?\n/);
  let inBlock = false;
  lines.forEach((rawLine, lineNo) => {
    let line = rawLine;
    if (inBlock) {
      const end = line.indexOf("*/");
      if (end < 0) return;
      line = " ".repeat(end + 2) + line.slice(end + 2);
      inBlock = false;
    }
    if (/^\s*#/.test(line)) return;
    let comment = null;
    let i = 0;
    while (i < line.length) {
      const ch = line[i];
      if (ch === "/" && line[i + 1] === "/") {
        comment = line.slice(i + 2).trim();
        break;
      }
      if (ch === "/" && line[i + 1] === "*") {
        const end = line.indexOf("*/", i + 2);
        if (end < 0) {
          inBlock = true;
          break;
        }
        i = end + 2;
        continue;
      }
      if (/\s/.test(ch)) {
        i++;
        continue;
      }
      const ident = /^[A-Za-z_][A-Za-z0-9_]*/.exec(line.slice(i));
      if (ident) {
        tokens.push({ v: ident[0], line: lineNo });
        i += ident[0].length;
        continue;
      }
      const num = /^(0[xX][0-9a-fA-F]+|\d+)[uUlL]*/.exec(line.slice(i));
      if (num) {
        tokens.push({ v: num[0], num: Number(num[1]), line: lineNo });
        i += num[0].length;
        continue;
      }
      tokens.push({ v: ch, line: lineNo });
      i++;
    }
    if (comment) {
      // Attach a trailing comment to the last token on this line.
      for (let t = tokens.length - 1; t >= 0 && tokens[t].line === lineNo; t--) {
        if (tokens[t].v === ";") {
          tokens[t].comment = comment;
          break;
        }
      }
    }
  });
  return tokens;
}

// ---------------------------------------------------------------- parser

const constants = new Map();
const structs = new Map();

function parseHeader(src) {
  const toks = tokenize(src);
  let p = 0;
  const peek = (o = 0) => toks[p + o]?.v;
  const next = () => toks[p++];
  const expect = (v) => {
    const t = next();
    if (t?.v !== v) throw new Error(`expected '${v}' but got '${t?.v}' on line ${t ? t.line + 1 : "EOF"}`);
    return t;
  };

  const evalExpr = (tokens) => {
    const expr = tokens
      .map((t) => {
        if (t.num !== undefined) return String(t.num);
        if (constants.has(t.v)) return String(constants.get(t.v));
        if (/^[-+*/()<>|&~ ]$/.test(t.v)) return t.v;
        throw new Error(`unknown constant '${t.v}' on line ${t.line + 1}`);
      })
      .join(" ");
    return Function(`"use strict"; return (${expr});`)();
  };

  const skipBalanced = (open, close) => {
    let depth = 0;
    do {
      const t = next();
      if (t.v === open) depth++;
      else if (t.v === close) depth--;
    } while (depth > 0);
  };

  const parseEnum = () => {
    expect("enum");
    if (peek() !== "{") next();
    expect("{");
    let value = -1;
    while (peek() !== "}") {
      const name = next().v;
      if (peek() === "=") {
        next();
        const exprToks = [];
        while (peek() !== "," && peek() !== "}") exprToks.push(next());
        value = evalExpr(exprToks);
      } else {
        value += 1;
      }
      constants.set(name, value);
      if (peek() === ",") next();
    }
    expect("}");
    if (peek() === ";") next();
  };

  // Parses a struct body. Returns null when the struct has methods or pointers,
  // which means it is not a plain on-disk record (e.g. talk_proc).
  const parseStructBody = () => {
    expect("{");
    const members = [];
    let plain = true;
    while (peek() !== "}") {
      if (peek() === "struct" && (peek(1) === "{" || peek(2) === "{")) {
        next();
        let name = null;
        if (peek() !== "{") name = next().v;
        const body = parseStructBody();
        if (!body) plain = false;
        if (name && body) structs.set(name, body);
        if (peek() === ";") {
          next();
          continue;
        }
        parseDeclarators({ struct: body }, members);
        continue;
      }
      if (peek() === "enum") {
        parseEnum();
        continue;
      }
      // Member functions, constructors and anything else we cannot lay out.
      const start = p;
      let sawParen = false;
      let q = p;
      while (toks[q] && toks[q].v !== ";" && toks[q].v !== "{" && toks[q].v !== "}") {
        if (toks[q].v === "(") sawParen = true;
        q++;
      }
      if (sawParen) {
        plain = false;
        p = start;
        while (peek() !== "(") next();
        skipBalanced("(", ")");
        while (peek() !== "{" && peek() !== ";") next();
        if (peek() === "{") skipBalanced("{", "}");
        else next();
        continue;
      }
      const typeWords = [];
      if (peek() === "struct") next();
      while (toks[p + 1] && toks[p + 1].v !== ";" && toks[p + 1].v !== "[" && toks[p + 1].v !== "," && toks[p].v !== "*") {
        typeWords.push(next().v);
      }
      const typeName = typeWords.join(" ");
      if (peek() === "*") {
        plain = false;
        while (peek() !== ";") next();
        next();
        continue;
      }
      parseDeclarators({ name: typeName }, members);
    }
    expect("}");
    return plain ? members : null;
  };

  const parseDeclarators = (type, members) => {
    for (;;) {
      const nameTok = next();
      const dims = [];
      while (peek() === "[") {
        next();
        const exprToks = [];
        while (peek() !== "]") exprToks.push(next());
        next();
        dims.push(evalExpr(exprToks));
      }
      members.push({ name: nameTok.v, type, dims });
      if (peek() === ",") {
        next();
        continue;
      }
      const end = expect(";");
      if (end.comment) members[members.length - 1].comment = end.comment;
      for (let m = members.length - 2; m >= 0 && members[m].comment === undefined && members[m].type === type; m--) {
        members[m].comment = end.comment;
      }
      return;
    }
  };

  while (p < toks.length) {
    if (peek() === "enum") {
      parseEnum();
    } else if (peek() === "struct" && peek(2) === "{") {
      next();
      const name = next().v;
      const body = parseStructBody();
      if (body) structs.set(name, body);
      if (peek() === ";") next();
    } else if (peek() === "typedef") {
      while (peek() !== ";") next();
      next();
    } else {
      next();
    }
  }
}

// ---------------------------------------------------------------- layout

const alignUp = (n, a) => Math.ceil(n / a) * a;

// Returns { t: TypeDef, size, align } for a member type (before array dims).
function layoutType(type, structName) {
  if (type.struct) return layoutStruct(type.struct);
  if (PRIMITIVES[type.name]) {
    const prim = PRIMITIVES[type.name];
    return { t: { k: prim.k }, size: prim.size, align: Math.min(prim.size, PACK), base: type.name };
  }
  if (structs.has(type.name)) return layoutStruct(structs.get(type.name));
  throw new Error(`unknown type '${type.name}' in ${structName}`);
}

function layoutStruct(members, structName = "?") {
  const fields = [];
  let offset = 0;
  let maxAlign = 1;
  for (const m of members) {
    const elem = layoutType(m.type, structName);
    let t = elem.t;
    let size = elem.size;
    const dims = [...m.dims];
    // A trailing dimension on a character type is a fixed-size string.
    if (dims.length && elem.base === "namechar") {
      t = { k: "wstr", n: dims.pop() };
      size = t.n * 2;
    } else if (dims.length && (elem.base === "char" || elem.base === "signed char")) {
      t = { k: "str", n: dims.pop() };
      size = t.n;
    } else if (dims.length && elem.base === "unsigned char") {
      t = { k: "bytes", n: dims.pop() };
      size = t.n;
    }
    for (let d = dims.length - 1; d >= 0; d--) {
      t = { k: "array", n: dims[d], stride: size, t };
      size *= dims[d];
    }
    offset = alignUp(offset, elem.align);
    const field = { name: m.name, off: offset, t };
    if (m.comment) field.c = m.comment;
    fields.push(field);
    offset += size;
    maxAlign = Math.max(maxAlign, elem.align);
  }
  const size = alignUp(offset, maxAlign);
  return { t: { k: "struct", fields }, size, align: maxAlign };
}

// ---------------------------------------------------------------- list order

// Walks elementdataman::load_data in order: each `x_array.load(file)` is a
// list, each `fseek(file, 8, SEEK_CUR)` an 8-byte checksum slot, and each
// `fread(&tag, …)` an exporter/tag block (an exporter block also reads a
// trailing timestamp before the next list).
function parseLoadOrder(cpp, header) {
  const typeOfArray = new Map();
  for (const m of header.matchAll(/array\s*<\s*(\w+)\s*>\s*(\w+)_array\s*;/g)) typeOfArray.set(m[2], m[1]);
  const start = cpp.indexOf("int elementdataman::load_data");
  if (start < 0) throw new Error("elementdataman::load_data not found");
  const body = cpp.slice(start, cpp.indexOf("talk_proc", start));
  const lists = [];
  const markers = [];
  const events = /(\w+)_array\.load\(file\)|fseek\(file,\s*8,\s*SEEK_CUR\)|fread\(&tag\b|fread\(&t\s*,/g;
  for (const m of body.matchAll(events)) {
    if (m[1]) {
      const struct = typeOfArray.get(m[1]);
      if (!struct) throw new Error(`no array<> declaration for ${m[1]}_array`);
      lists.push({ key: m[1], struct });
    } else if (m[0].startsWith("fseek")) {
      markers.push({ before: lists.length, kind: "checksum" });
    } else if (m[0].startsWith("fread(&tag")) {
      markers.push({ before: lists.length, kind: "tag" });
    } else {
      const last = markers.at(-1);
      if (last?.kind === "tag" && last.before === lists.length) last.kind = "exporter";
    }
  }
  return { lists, markers };
}

const humanize = (key) => key.split("_").map((w) => w[0].toUpperCase() + w.slice(1)).join(" ");

// ---------------------------------------------------------------- main

const exptypes = readSource("exptypes.h");
parseHeader(exptypes);
const versionMatch = /#define\s+ELEMENTDATA_VERSION\s+(0x[0-9a-fA-F]+|\d+)/.exec(exptypes);
const version = Number(versionMatch[1]) & 0xffff;

const order = parseLoadOrder(readSource("elementdataman.cpp"), readSource("elementdataman.h"));
const lists = order.lists.map(({ key, struct }) => {
  const members = structs.get(struct);
  if (!members) throw new Error(`struct ${struct} not found or not plain`);
  const { t, size } = layoutStruct(members, struct);
  return { key, name: humanize(key), struct, size, fields: t.fields };
});

const id = layoutId;
const layout = {
  id,
  version,
  source: `Server template sources (${path.basename(path.resolve(templateDir, "../../.."))}: exptypes.h, elementdataman.cpp)`,
  markers: order.markers,
  lists,
};

if (validatePath) {
  const problems = validateLayout(layout, validatePath);
  if (problems.length) {
    problems.forEach((p) => console.error("  " + p));
    console.error(`validation failed: ${problems.length} problem(s)`);
    process.exit(2);
  }
  console.log(`validated ${lists.length} list layouts against ${validatePath}`);
}

const outDir = writeLayout(layout);
console.log(`wrote ${outDir}: v${version}, ${lists.length} lists, markers ${order.markers.map((m) => m.kind[0] + m.before).join(" ")}`);
