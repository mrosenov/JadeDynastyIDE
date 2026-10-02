// Field tree nodes are addressed by index paths such as "3/0/2".

import type { FieldNode } from "./types";

export type Path = string;

interface Row {
  node: FieldNode;
  path: Path;
  depth: number;
}

export function flatten(nodes: FieldNode[], expanded: Set<Path>, prefix = "", depth = 0, out: Row[] = []): Row[] {
  nodes.forEach((node, i) => {
    const path = prefix ? `${prefix}/${i}` : String(i);
    out.push({ node, path, depth });
    if (node.children && expanded.has(path)) flatten(node.children, expanded, path, depth + 1, out);
  });
  return out;
}

/** Path of the deepest node covering `offset`, plus all its ancestors. */
export function pathAt(nodes: FieldNode[], offset: number): Path[] {
  const found: Path[] = [];
  let level = nodes;
  let prefix = "";
  for (;;) {
    const i = level.findIndex((n) => offset >= n.off && offset < n.off + n.size);
    if (i < 0) return found;
    prefix = prefix ? `${prefix}/${i}` : String(i);
    found.push(prefix);
    const children = level[i].children;
    if (!children) return found;
    level = children;
  }
}

export function nodeAt(nodes: FieldNode[], path: Path): FieldNode | null {
  let node: FieldNode | null = null;
  let level: FieldNode[] | undefined = nodes;
  for (const part of path.split("/")) {
    node = level?.[Number(part)] ?? null;
    if (!node) return null;
    level = node.children;
  }
  return node;
}

export function allPaths(nodes: FieldNode[], prefix = "", out: Path[] = []): Path[] {
  nodes.forEach((n, i) => {
    const path = prefix ? `${prefix}/${i}` : String(i);
    if (n.children) {
      out.push(path);
      allPaths(n.children, path, out);
    }
  });
  return out;
}
