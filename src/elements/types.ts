// Mirrors the serialized types in src-tauri/src/elements.

export type LayoutFit = "exact" | "approx" | "partial" | "none";

export type SegmentKind = "header" | "list" | "checksum" | "tag" | "talk";

export interface Segment {
  kind: SegmentKind;
  offset: number;
  size: number;
}

export interface ListSummary {
  index: number;
  name: string;
  key: string | null;
  structName: string | null;
  approxName: boolean;
  itemSize: number;
  count: number;
  offset: number;
  layout: LayoutFit;
}

export interface FileSummary {
  path: string;
  fileSize: number;
  version: number;
  rawVersion: number;
  timestamp: number;
  exporter: string | null;
  layoutSignature: string;
  profileVersion: number | null;
  profileSource: string | null;
  profileExact: boolean;
  talkCount: number;
  lists: ListSummary[];
  segments: Segment[];
}

export interface RecordRow {
  index: number;
  id: number;
  name: string;
}

export interface FieldNode {
  name: string;
  off: number;
  size: number;
  ty: string;
  value?: string;
  hint?: string;
  comment?: string;
  children?: FieldNode[];
  unknown?: boolean;
}

export interface RecordDetail {
  list: number;
  index: number;
  fileOffset: number;
  layout: LayoutFit;
  layoutSize: number | null;
  bytes: number[];
  nodes: FieldNode[];
}
