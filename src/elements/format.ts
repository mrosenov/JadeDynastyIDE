import type { LayoutFit } from "./types";

export const hex = (n: number, pad = 0) => "0x" + n.toString(16).toUpperCase().padStart(pad, "0");

export const count = (n: number) => n.toLocaleString("en-US");

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

export const LAYOUT_LABEL: Record<LayoutFit, string> = {
  exact: "Exact layout",
  approx: "Borrowed layout",
  partial: "Partial layout",
  none: "No layout",
};

export const LAYOUT_HELP: Record<LayoutFit, string> = {
  exact: "Field layout generated for this exact version and record size.",
  approx: "Field layout borrowed from a neighbouring version with the same record size. Most likely correct.",
  partial:
    "Records are larger than the known layout. Known fields are decoded from the start; the extra bytes are shown as unknown. Fields may be shifted if the game inserted new ones mid-struct.",
  none: "No layout is known for this list. Bytes are shown as raw int32 values.",
};
