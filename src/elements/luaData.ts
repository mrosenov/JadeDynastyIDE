import { parse } from "luaparse";

/** A literal value of a script: numbers, texts, booleans, nil, tables, or (for anything else) its source text. */
export type LuaValue = number | string | boolean | null | { expr: string } | LuaTable;
export interface LuaTable {
  table: true;
  /** The array part, in order. */
  list: LuaValue[];
  /** Keyed fields (`a = 1`, `["a"] = 1`, `[3] = 1`). */
  fields: { key: string; value: LuaValue }[];
}

export interface LuaFunction {
  name: string;
  params: string[];
  line: number;
  endLine: number;
  /** The comment on the function's first line or right above it. */
  comment: string | null;
}

/** An assignment of literal data (`x = 5`, `local t = {1, 2}`, `Out_Prize[1][4713] = 100`). */
export interface LuaEntry {
  /** The enclosing function's name ("" at the top level). */
  fn: string;
  /** Enclosing `if`/`elseif`/`else` and loop headers, outermost first. */
  conditions: string[];
  target: string;
  value: LuaValue;
  line: number;
  comment: string | null;
  local: boolean;
}

/** A `return` with what it returns, as source text. */
export interface LuaReturn {
  fn: string;
  conditions: string[];
  values: string[];
  line: number;
  comment: string | null;
}

export interface LuaScript {
  functions: LuaFunction[];
  entries: LuaEntry[];
  returns: LuaReturn[];
  /** The parser's message when the file is not valid Lua 5.1 (what gs would refuse with -11). */
  error: string | null;
}

// luaparse's nodes, loosely typed: only the fields used here.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Node = any;

const squash = (text: string) => text.replace(/\s+/g, " ").trim();

/** A Lua string literal's text (quotes and the common escapes removed). */
function stringOf(raw: string): string {
  const long = /^\[(=*)\[([\s\S]*)\]\1\]$/.exec(raw);
  if (long) return long[2].replace(/^\r?\n/, "");
  return raw.slice(1, -1).replace(/\\(n|t|r|\\|"|'|\d{1,3})/g, (_, code: string) => code === "n" ? "\n" : code === "t" ? "\t" : code === "r" ? "\r" : /^\d+$/.test(code) ? String.fromCharCode(Number(code)) : code);
}

/** Reads a script; never throws (a parse error is reported in `error`). */
export function readLuaScript(text: string): LuaScript {
  const script: LuaScript = { functions: [], entries: [], returns: [], error: null };
  let ast: Node;
  try {
    ast = parse(text, { comments: true, locations: true, ranges: true, scope: false, luaVersion: "5.1" });
  } catch (problem) {
    script.error = String((problem as Error).message ?? problem);
    return script;
  }
  const source = (node: Node) => squash(text.slice(node.range[0], node.range[1]));
  // Comments by line: the text after the dashes.
  const comments = new Map<number, string>();
  for (const comment of ast.comments ?? []) {
    const value = squash(String(comment.value ?? "")).replace(/^[-/\s]+/, "");
    if (!value) continue;
    const line = comment.loc.start.line;
    comments.set(line, comments.has(line) ? `${comments.get(line)} ${value}` : value);
  }
  const commentAt = (line: number) => comments.get(line) ?? null;

  const value = (node: Node): LuaValue => {
    switch (node?.type) {
      case "NumericLiteral": return node.value;
      case "StringLiteral": return stringOf(node.raw);
      case "BooleanLiteral": return node.value;
      case "NilLiteral": return null;
      case "UnaryExpression":
        if (node.operator === "-" && node.argument.type === "NumericLiteral") return -node.argument.value;
        return { expr: source(node) };
      case "TableConstructorExpression": {
        const table: LuaTable = { table: true, list: [], fields: [] };
        for (const field of node.fields) {
          if (field.type === "TableValue") table.list.push(value(field.value));
          else if (field.type === "TableKeyString") table.fields.push({ key: field.key.name, value: value(field.value) });
          else table.fields.push({ key: field.key.type === "StringLiteral" ? stringOf(field.key.raw) : source(field.key), value: value(field.value) });
        }
        return table;
      }
      default: return { expr: source(node) };
    }
  };
  /** Literal data: numbers, texts, tables holding at least one literal. */
  const isData = (node: Node): boolean => {
    if (!node) return false;
    if (["NumericLiteral", "StringLiteral", "BooleanLiteral"].includes(node.type)) return true;
    if (node.type === "UnaryExpression") return node.operator === "-" && node.argument.type === "NumericLiteral";
    if (node.type === "TableConstructorExpression") return node.fields.length > 0 && node.fields.some((field: Node) => isData(field.value) || field.value.type === "TableConstructorExpression");
    return false;
  };

  const walk = (body: Node[], fn: string, conditions: string[]) => {
    for (const statement of body) visit(statement, fn, conditions);
  };
  const declare = (node: Node, name: string) => {
    const line = node.loc.start.line;
    script.functions.push({ name, params: (node.parameters ?? []).map((parameter: Node) => parameter.name ?? "..."), line, endLine: node.loc.end.line, comment: commentAt(line) ?? commentAt(line - 1) });
    walk(node.body, name, []);
  };
  const visit = (node: Node, fn: string, conditions: string[]) => {
    switch (node.type) {
      case "FunctionDeclaration":
        declare(node, node.identifier ? source(node.identifier) : "(function)");
        break;
      case "LocalStatement":
      case "AssignmentStatement":
        node.variables.forEach((variable: Node, index: number) => {
          const init = node.init[index];
          if (!init) return;
          if (init.type === "FunctionDeclaration") { declare(init, source(variable)); return; }
          if (!isData(init)) return;
          const line = init.loc.end.line;
          script.entries.push({ fn, conditions, target: source(variable), value: value(init), line: variable.loc.start.line, comment: commentAt(line) ?? commentAt(variable.loc.start.line), local: node.type === "LocalStatement" });
        });
        break;
      case "IfStatement": {
        for (const clause of node.clauses) {
          const label = clause.type === "ElseClause" ? "else" : `${clause.type === "IfClause" ? "if" : "elseif"} ${source(clause.condition)}`;
          walk(clause.body, fn, [...conditions, label]);
        }
        break;
      }
      case "WhileStatement": walk(node.body, fn, [...conditions, `while ${source(node.condition)}`]); break;
      case "RepeatStatement": walk(node.body, fn, [...conditions, `repeat until ${source(node.condition)}`]); break;
      case "DoStatement": walk(node.body, fn, conditions); break;
      case "ForNumericStatement": walk(node.body, fn, [...conditions, `for ${node.variable.name} = ${source(node.start)}, ${source(node.end)}${node.step ? `, ${source(node.step)}` : ""}`]); break;
      case "ForGenericStatement": walk(node.body, fn, [...conditions, `for ${node.variables.map((variable: Node) => variable.name).join(", ")} in ${node.iterators.map(source).join(", ")}`]); break;
      case "ReturnStatement":
        script.returns.push({ fn, conditions, values: node.arguments.map(source), line: node.loc.start.line, comment: commentAt(node.loc.start.line) });
        break;
      default: break;
    }
  };
  walk(ast.body, "", []);
  return script;
}

/** Numbers of a table's array part (non-numbers as NaN). */
export const numbersOf = (value: LuaValue): number[] => typeof value === "object" && value !== null && "table" in value ? value.list.map((entry) => typeof entry === "number" ? entry : NaN) : [];

/** A value as short text. */
export function showValue(value: LuaValue): string {
  if (value === null) return "nil";
  if (typeof value === "string") return `"${value}"`;
  if (typeof value !== "object") return String(value);
  if ("expr" in value) return value.expr;
  const parts = [...value.list.map(showValue), ...value.fields.map((field) => `${field.key} = ${showValue(field.value)}`)];
  return `{${parts.join(", ")}}`;
}

/** Shares of a weight list (0–1; all zero: the first gets everything, as the scripts' `*_Chance_Norm`). */
export function shares(weights: number[]): number[] {
  const total = weights.reduce((sum, weight) => sum + (Number.isFinite(weight) && weight > 0 ? weight : 0), 0);
  if (total === 0) return weights.map((_, index) => index === 0 ? 1 : 0);
  return weights.map((weight) => Number.isFinite(weight) && weight > 0 ? weight / total : 0);
}
