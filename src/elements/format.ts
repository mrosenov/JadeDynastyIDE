import type { LayoutFit } from "./types";

export const hex = (n: number, pad = 0) => "0x" + n.toString(16).toUpperCase().padStart(pad, "0");

export const count = (n: number) => n.toLocaleString("en-US");

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

export const LAYOUT_LABEL: Record<LayoutFit, string> = {
  exact: "Exact",
  partial: "Partial",
  borrowed: "Borrowed",
  grown: "Grown struct",
  named: "Name only",
  none: "Unknown",
};

export function layoutHelp(fit: LayoutFit, layoutId: string | null): string {
  const from = layoutId ? ` from ${layoutId}` : "";
  switch (fit) {
    case "exact":
      return `Field layout${from}, made for this version; the record size matches.`;
    case "partial":
      return `Field layout${from} covers the start of each record; the remaining bytes are shown as unknown.`;
    case "borrowed":
      return `Field layout borrowed${from}. Lists were matched by record size, so this is very likely correct.`;
    case "grown":
      return `This struct grew since${layoutId ? " " + layoutId : " the version it was borrowed from"}. Its known fields are decoded from the start and the new bytes are shown as unknown. If the game inserted fields mid-struct, later values are shifted.`;
    case "named":
      return "Only the list's name is known. Bytes are shown as raw int32 values.";
    case "none":
      return "No layout is known for this list. Bytes are shown as raw int32 values.";
  }
}
