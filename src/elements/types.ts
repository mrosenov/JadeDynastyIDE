// Mirrors the serialized types in src-tauri/src/elements.

export type LayoutFit = "exact" | "partial" | "borrowed" | "grown" | "named" | "none";

export type ParseMode = "layout" | "markers" | "detected";

export type SegmentKind = "header" | "list" | "checksum" | "exporter" | "tag" | "talk";

export interface Segment {
  kind: SegmentKind;
  offset: number;
  size: number;
  before: number | null;
}

export interface ListSummary {
  index: number;
  name: string;
  key: string | null;
  structName: string | null;
  itemSize: number;
  count: number;
  offset: number;
  layout: LayoutFit;
  layoutId: string | null;
  layoutSize: number | null;
  /** The definition comes from a user layout written in the schema editor. */
  custom: boolean;
}

export interface FileSummary {
  path: string;
  fileSize: number;
  version: number;
  rawVersion: number;
  timestamp: number;
  exporter: string | null;
  parseMode: ParseMode;
  layoutId: string | null;
  layoutSource: string | null;
  layoutUnverified: boolean;
  layoutCustom: boolean;
  markersFrom: string | null;
  talkCount: number;
  lists: ListSummary[];
  segments: Segment[];
}

export interface RecordRow {
  index: number;
  id: number;
  name: string;
  /** Path ID of the record's item icon (client icons). */
  icon?: number;
}

export interface FieldNode {
  name: string;
  off: number;
  size: number;
  ty: string;
  value?: string;
  hint?: string;
  /** Referenced record as [list, row]. */
  link?: [number, number];
  /** The NPC dialog this value opens (index into the dialogs). */
  talk?: number;
  display?: string;
  comment?: string;
  children?: FieldNode[];
  unknown?: boolean;
  /** A display group of fields rather than a value. */
  group?: boolean;
  /** Why a conditional type was chosen, e.g. "type = 7 → float". */
  cond?: string;
  /** Path ID of the item icon this value points at. */
  icon?: number;
  /** Key of the enum or mask naming this value. */
  set?: string;
}

export interface RecordDetail {
  list: number;
  index: number;
  fileOffset: number;
  layout: LayoutFit;
  layoutId: string | null;
  layoutSize: number | null;
  bytes: number[];
  nodes: FieldNode[];
  /** Path ID of the record's item icon. */
  icon: number | null;
}

// ---------------------------------------------------------------- schemas

/** A field type as stored in layouts (src-tauri/src/elements/format.rs). */
export type Ty =
  | { k: "i8" | "u8" | "bool" | "i16" | "u16" | "i32" | "u32" | "f32" | "f64" | "i64" | "u64" }
  | { k: "wstr" | "str" | "bytes"; n: number }
  | { k: "array"; n: number; stride: number; t: Ty }
  | { k: "struct"; fields: Field[] };

export interface Field {
  name: string;
  off: number;
  t: Ty;
  c?: string;
  e?: string;
  display?: string;
  refs?: string[];
  /** Display group shared by consecutive fields. */
  g?: string;
  /** Conditional types: the first matching rule decides how the field is read. */
  when?: TypeRule[];
}

/** "If sibling `field` is (not) one of `in`, read the field as `t`." */
export interface TypeRule {
  field: string;
  in: number[];
  not?: boolean;
  t: Ty;
}

export interface ListDef {
  key?: string;
  name: string;
  struct?: string;
  size?: number;
  fields?: Field[];
}

export interface ListSchema {
  list: number;
  itemSize: number;
  count: number;
  targetLayout: string;
  targetExists: boolean;
  def: ListDef | null;
  defLayout: string | null;
  fit: LayoutFit;
  custom: boolean;
  hasBuiltin: boolean;
}

export interface EnumInfo {
  key: string;
  label: string;
  flags: boolean;
  count: number;
}

export interface RefTarget {
  list: number;
  name: string;
  structName: string;
}

export interface SchemaContext {
  enums: EnumInfo[];
  /** Lists of the open file that refs can point at. */
  targets: RefTarget[];
  userDir: string;
  errors: string[];
}

/** The same list slot as defined by another layout. */
export interface ImportCandidate {
  layoutId: string;
  version: number;
  name: string;
  structName: string | null;
  /** Bytes the definition describes. */
  size: number;
  /** Record size of the list in the open file. */
  itemSize: number;
  def: ListDef;
}

// ---------------------------------------------------------------- settings

export interface Settings {
  clientDir: string | null;
  openOnStart: boolean;
}

export interface DataFile {
  name: string;
  path: string;
  size: number;
  kind: string;
  supported: boolean;
}

export interface PackageFile {
  name: string;
  path: string;
  size: number;
  parts: number;
}

export interface ClientInfo {
  root: string;
  elementDir: string;
  dataFiles: DataFile[];
  packages: PackageFile[];
  elementsPath: string | null;
  hasPathData: boolean;
  hasItemIcons: boolean;
}

export interface SettingsView {
  settings: Settings;
  client: ClientInfo | null;
  clientError: string | null;
  /** Part of icon URLs; changes when the client changes. */
  iconGeneration: number;
}

// ---------------------------------------------------------------- enums and masks

export type SetKind = "enum" | "mask";

/** builtin: shipped with the app; override: a built-in set you changed; user: created by you. */
/** "deleted": a built-in set the user deleted (listed so it can be restored). */
export type SetOrigin = "builtin" | "override" | "user" | "deleted";

export interface EnumValue {
  value: number;
  label: string;
  description?: string;
}

export interface MaskFlag {
  /** Bit index 0…63 (value 1 << bit). */
  bit: number;
  label: string;
  description?: string;
}

export interface NamedSet {
  key: string;
  label: string;
  values?: EnumValue[];
  flags?: MaskFlag[];
}

export interface SetSummary {
  key: string;
  label: string;
  kind: SetKind;
  origin: SetOrigin;
  count: number;
}

export interface SetDetail {
  kind: SetKind;
  set: NamedSet;
  origin: SetOrigin;
  builtin: NamedSet | null;
  usage: string[];
}

export interface SetsChanged {
  sets: SetSummary[];
  summary: FileSummary | null;
}

// ---------------------------------------------------------------- referenced by

export interface Referrer {
  list: number;
  row: number;
  id: number;
  name: string;
  /** Field path in the referring record, e.g. "pages[2].id_goods[5]". */
  field: string;
  /** declared: the field's refs name this list; id: matched by field name and ID space. */
  how: "declared" | "id";
  icon?: number;
}

export interface ReferencedBy {
  id: number;
  referrers: Referrer[];
  truncated: boolean;
}

/** A record found by Find. */
export interface FindHit {
  list: number;
  index: number;
  id: number;
  name: string;
  icon?: number;
  /** id: the record's ID is the query; name: its name contains it. */
  how: "id" | "name";
}

export interface FindResult {
  hits: FindHit[];
  /** Matches in all, including those past the limit. */
  total: number;
}

/** An NPC dialog in the list of dialogs. */
export interface TalkSummary {
  index: number;
  id: number;
  title: string;
  windows: number;
  options: number;
  /** Records that open the dialog (through `id_dialog`). */
  users: number;
  usedBy?: string;
}

export interface TalkOption {
  /** A child window, or a function when the top bit is set. */
  id: number;
  text: string;
  param: number;
}

export interface TalkWindow {
  id: number;
  /** 0xFFFFFFFF for the root window. */
  parent: number;
  text: string;
  options: TalkOption[];
}

export interface TalkUser {
  list: number;
  row: number;
  id: number;
  name: string;
}

export interface TalkDetail {
  index: number;
  id: number;
  /** The dialog's prompt ("RootNode" in most). */
  text: string;
  windows: TalkWindow[];
  offset: number;
  size: number;
  users: TalkUser[];
}
