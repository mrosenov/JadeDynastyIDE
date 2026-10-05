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
  /** The item's name colour in the game ("#rrggbb"), when not white. */
  color?: string;
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
  /** Longer client text shown when hovering a skill or buff hint. */
  description?: string;
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
  /** Optional colour chosen for a group or struct heading. */
  color?: string;
  /** Why a conditional type was chosen, e.g. "type = 7 → float". */
  cond?: string;
  /** Path ID of the item icon this value points at. */
  icon?: number;
  /** Key of the enum or mask naming this value. */
  set?: string;
  /** On-demand source for choosing this integer value. */
  picker?: "reference" | "skill" | "buff" | "title" | "path" | "icon" | "dialog";
}

export interface PickerEntry {
  value: number;
  name: string;
  detail?: string;
  description?: string;
  /** Path ID of an item icon. */
  icon?: number;
  list?: number;
  row?: number;
}

export interface PickerResult {
  kind: string;
  title: string;
  scope: string;
  current: number | null;
  entries: PickerEntry[];
  total: number;
  page: number;
  pageSize: number;
}

export interface PickerRequest {
  list: number;
  row: number;
  off: number;
  query: string;
  page: number;
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
  /** The record as the file was opened, when edits changed it. */
  original?: number[];
  /** Created by an edit (a clone). */
  added?: boolean;
  /** The client's description of the record (configs.pck). */
  gameText?: { text: string; source: string };
  /** The name colour in the game, when not white. */
  nameColor?: string;
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
  /** Optional colour of this struct heading. */
  color?: string;
  /** Optional colour of the display group named by `g`. */
  gc?: string;
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

export type Theme = "system" | "light" | "dark";

export interface Settings {
  clientDir: string | null;
  openOnStart: boolean;
  theme: Theme;
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

/** Translation-only changes for one dialog. Numeric control data is omitted. */
export interface TalkTextEdit {
  text: string;
  windows: { text: string; options: string[] }[];
}

// ---------------------------------------------------------------- advanced search

export type SearchOp =
  | "eq"
  | "ne"
  | "lt"
  | "le"
  | "gt"
  | "ge"
  | "in"
  | "not_in"
  | "contains"
  | "starts"
  | "ends"
  | "has_flags"
  | "lacks_flags"
  | "empty"
  | "not_empty";

export interface SearchCondition {
  /** A field name ("proc_type"), or a dotted path ("addons.id"). */
  field: string;
  op: SearchOp;
  value: string;
}

export type SearchQuery =
  | {
      mode: "value";
      value: string;
      kind: "auto" | "int" | "float" | "text" | "hex";
      list: number | null;
      includeUnknown: boolean;
      caseSensitive: boolean;
    }
  | { mode: "conditions"; conditions: SearchCondition[]; matchAll: boolean; list: number | null };

export interface SearchMatch {
  /** "addons[2].id", or "+0x01F4" for bytes no layout describes. */
  field: string;
  off: number;
  value: string;
  label?: string;
}

export interface SearchHit {
  list: number;
  row: number;
  id: number;
  name: string;
  icon?: number;
  matches: SearchMatch[];
}

export interface SearchReport {
  hits: SearchHit[];
  matchedRecords: number;
  matchedLists: number;
  scannedLists: number;
  scannedRecords: number;
  truncated: boolean;
  elapsedMs: number;
}

export interface SearchFieldName {
  name: string;
  lists: number;
  kind: "int" | "float" | "text" | "bytes";
  set?: string;
}

// ---------------------------------------------------------------- problems

export type ProblemSeverity = "error" | "warning" | "info";

export type ProblemKind =
  | "duplicate_id"
  | "shadowed_id"
  | "broken_ref"
  | "missing_dialog"
  | "dialog_window"
  | "missing_path"
  | "unterminated_text"
  | "unknown_enum"
  | "unnamed_bits"
  | "layout";

export interface Problem {
  kind: ProblemKind;
  /** The record (no list: a dialog, with `talk`). */
  list?: number;
  /** No row: the whole list. */
  row?: number;
  id: number;
  name: string;
  icon?: number;
  field?: string;
  off?: number;
  message: string;
  talk?: number;
}

export interface ProblemKindSummary {
  kind: ProblemKind;
  severity: ProblemSeverity;
  title: string;
  description: string;
  count: number;
}

export interface ProblemReport {
  kinds: ProblemKindSummary[];
  problems: Problem[];
  truncated: boolean;
  pathsChecked: boolean;
  elapsedMs: number;
}

// ---------------------------------------------------------------- coverage, export, compare

export interface CoverageRow {
  index: number;
  name: string;
  structName: string | null;
  itemSize: number;
  count: number;
  fit: LayoutFit;
  layoutId: string | null;
  custom: boolean;
  /** Bytes of named fields. */
  described: number;
  /** Bytes of placeholder fields ("unknown_12"). */
  placeholder: number;
  /** Bytes no field covers. */
  undefined: number;
  fields: number;
}

export type ExportSource =
  | { from: "item"; list: number; row: number }
  | { from: "list"; list: number }
  | { from: "search"; query: SearchQuery };

export interface Exported {
  path: string;
  records: number;
  columns: number;
}

export interface ImportReport {
  token: string;
  total: number;
  matched: number;
  changing: number;
  adding: number;
  sourceVersion: number | null;
  unchanged: number;
  rejected: number;
  fields: number;
  changes: { sourceRow: number; list: number; id: number; field: string; old: string; new: string }[];
  additions: { sourceRow: number; list: number; id: number; name: string }[];
  issues: { sourceRow: number; message: string }[];
  state: EditState | null;
}

export interface CompareFile {
  path: string;
  version: number;
  timestamp: number;
  fileSize: number;
  lists: number;
  records: number;
  talks: number;
}

export interface ComparePair {
  this: number | null;
  other: number | null;
  name: string;
  structName?: string;
  thisCount: number;
  otherCount: number;
  thisSize: number;
  otherSize: number;
  onlyThis: number;
  onlyOther: number;
  changed: number;
  canCopyRecordsFromOther: boolean;
  copyRecordsReason?: string;
}

export interface CompareSummary {
  this: CompareFile;
  other: CompareFile;
  lists: ComparePair[];
  /** NPC dialogs: only in this file, only in the other, changed. */
  talks: [number, number, number];
}

export interface CompareRecord {
  row: number;
  id: number;
  name: string;
  icon?: number;
}

export interface FieldChange {
  field: string;
  this: string | null;
  other: string | null;
  copyable: boolean;
}

export interface ChangedRecord extends CompareRecord {
  otherRow: number;
  otherName?: string;
  fields: FieldChange[];
}

export interface ListDiff {
  onlyThis: CompareRecord[];
  onlyOther: CompareRecord[];
  changed: ChangedRecord[];
  truncated: boolean;
}

export interface CompareCopyRequest {
  thisList: number;
  otherList: number;
  fields: { thisRow: number; otherRow: number; fields: string[] }[];
  /** Rows in the compared file to append to the open file. */
  records: number[];
}

// ---------------------------------------------------------------- editing

export interface FieldEdit {
  /** Offset of the field in the record. */
  off: number;
  /** The new value as text (numbers in decimal or 0x hex). */
  value: string;
}

export interface EditState {
  /** Label of the edit Undo would take back. */
  undo?: string;
  redo?: string;
  /** Records of the opened file that differ from it: [list, row]. */
  changed: [number, number][];
  /** Records created by edits (clones): [list, row]. */
  added: [number, number][];
  /** Records of the opened file deleted: [list, count]. */
  deleted: [number, number][];
  /** Dialog indexes whose title, window text or option labels differ. */
  changedTalks: number[];
  /** How the last action moved rows, in order. */
  shifts: { list: number; at: number; delta: number; count: number }[];
  /** For a clone: where the new record is. */
  created?: [number, number];
  /** When the file was last saved (unix seconds); the records above count from then. */
  lastSaved?: number;
}

export type ChecksumStatus = "valid" | "mismatch" | "not_stored" | "no_path_data" | "no_slots";

/** The digest the client checks, and how the file on disk fares. */
export interface ChecksumCheck {
  status: ChecksumStatus;
  /** Digest slots in the file (the client reads four). */
  slots: number;
  pathData?: string;
  /** "chosen", "next to the file" or "client folder". */
  pathDataFrom?: string;
  /** The digest stored in the file on disk. */
  stored?: string;
  /** The digest that file should carry with this path.data. */
  expected?: string;
  checked?: string;
}

export interface SaveOptions {
  path: string;
  /** path.data for the digest; found by itself when not given. */
  pathData?: string;
  /** Keep a copy of the replaced file (once per file and session). */
  backup: boolean;
  /** Replace the open file even if another program changed it since it was read. */
  replaceChanged?: boolean;
}

/** What a save would do. */
export interface SavePlan {
  path: string;
  replaces: boolean;
  sameFile: boolean;
  /** The open file was changed by another program since it was read. */
  changedOnDisk: boolean;
  readOnly: boolean;
  size: number;
  changed: number;
  added: number;
  deleted: number;
  dialogs: number;
  checksum: ChecksumCheck;
  backup?: string;
}

export interface SaveReport {
  path: string;
  size: number;
  digest?: string;
  pathData?: string;
  backup?: string;
  timestamp: number;
}

export interface Saved {
  report: SaveReport;
  summary: FileSummary;
  state: EditState;
}

export interface FieldDiff {
  field: string;
  off: number;
  old: string;
  new: string;
}

export interface HistoryRecord {
  list: number;
  /** The row now (absent: the record is deleted). */
  row?: number;
  action: "edit" | "clone" | "import" | "copy" | "delete";
  id: number;
  name: string;
  icon?: number;
  fields: FieldDiff[];
}

export interface HistoryEntry {
  id: number;
  label: string;
  /** Unix time in milliseconds. */
  time: number;
  /** Undone: Redo would apply it again. */
  undone: boolean;
  /** The edit this one reverted. */
  reverts?: number;
  /** A later edit that reverted this one. */
  revertedBy?: number;
  /** When it was reverted (unix ms). */
  revertedAt?: number;
  /** The file was last saved after this edit (unix seconds). */
  savedAt?: number;
  records: HistoryRecord[];
}

// ---------------------------------------------------------------- bulk edit

export type BulkOp = "set" | "add" | "subtract" | "multiply" | "set_flags" | "clear_flags";

export interface BulkEdit {
  query: SearchQuery;
  /** Only these records ([list, row]) instead of every match. */
  records?: [number, number][];
  /** A field name ("proc_type") or an exact path ("addons[2].id"). */
  field: string;
  op: BulkOp;
  value: string;
}

export interface BulkSample {
  list: number;
  row: number;
  id: number;
  name: string;
  field: string;
  old: string;
  new: string;
  oldLabel?: string;
  newLabel?: string;
  error?: string;
}

export interface BulkReport {
  matched: number;
  changing: number;
  unchanged: number;
  skipped: number;
  skippedLists: string[];
  failed: number;
  samples: BulkSample[];
  state?: EditState;
}
