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
  /** Path ID of a standalone image in a client package. */
  image?: number;
  /** Key of the enum or mask naming this value. */
  set?: string;
  /** On-demand source for choosing this integer value. */
  picker?: "reference" | "skill" | "buff" | "title" | "path" | "icon" | "image" | "dialog";
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

/** A locally validated AI proposal. It remains unsaved until the user saves it in the schema editor. */
export interface LayoutAnalysis {
  summary: string;
  confidence: number;
  warnings: string[];
  definition: ListDef;
  referenceList: number;
  matchedRecords: number;
}

// ---------------------------------------------------------------- settings

export type Theme = "system" | "light" | "dark";

export interface Settings {
  clientDir: string | null;
  openOnStart: boolean;
  theme: Theme;
  aiEndpoint: string | null;
  aiModel: string | null;
  aiApiKey: string | null;
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

// ---------------------------------------------------------------- path.data

export interface PathDataRow {
  id: number;
  path: string;
}

export interface PathDataFile {
  path: string;
  size: number;
  token: string;
  rows: PathDataRow[];
}

export interface PathDataSaveRequest {
  openedPath: string;
  targetPath: string;
  token: string;
  rows: PathDataRow[];
  backup: boolean;
  replaceChanged: boolean;
}

export interface PathDataSaveReport {
  path: string;
  size: number;
  rows: number;
  token: string;
  backup?: string;
  siblingElements?: string;
  clientReloaded: boolean;
}

export interface PathDataJsonReport {
  path: string;
  rows: number;
}

export interface PathDataJsonImport {
  rows: PathDataRow[];
  sourcePath?: string;
  exportedAt?: string;
}

// ---------------------------------------------------------------- tasks.data

export interface TaskRootSummary {
  index: number;
  /** Zero-based numbered pack position (`tasks.data${pack + 1}`). */
  pack: number;
  /** Zero-based root position inside the pack. */
  root: number;
  id: number;
  name: string;
  childCount: number;
  byteSize: number;
}

export interface TasksFileSummary {
  path: string;
  version: number;
  exportVersion: number;
  rootCount: number;
  packCount: number;
  /** Index and numbered packs together. */
  size: number;
  roots: TaskRootSummary[];
  /** This version was opened with a user layout that passed exact whole-file verification. */
  userLayout: boolean;
}

export interface TaskSearchEntry {
  pack: number;
  root: number;
  path: number[];
  id: number;
  name: string;
  childCount: number;
}

export interface TaskSearchReport {
  indexed: boolean;
  error?: string;
  total: number;
  matches: TaskSearchEntry[];
}

export interface TaskSourceInfo {
  path: string;
  version: number;
  exportVersion: number;
  rootCount: number;
  packCount: number;
  size: number;
  supported: boolean;
  closestVersion: number;
  supportedVersions: number[];
}

export interface TaskSourceVersion {
  version: number;
  supported: boolean;
}

export interface TaskPackCoverage {
  pack: number;
  roots: number;
  exactRoots: number;
  trailingRoots: number;
  failedRoots: number;
  bytes: number;
  decodedBytes: number;
}

export interface TaskAnalysisIssue {
  pack: number;
  root: number;
  rootBytes: number;
  offset: number;
  kind: "failed" | "trailing" | "round_trip";
  message: string;
}

export interface TaskAnalysisReport {
  source: TaskSourceInfo;
  baselineVersion: number;
  exactRoots: number;
  trailingRoots: number;
  failedRoots: number;
  totalBytes: number;
  decodedBytes: number;
  rootCoverage: number;
  byteCoverage: number;
  exactRoundTrip: boolean;
  firstIssue?: TaskAnalysisIssue;
  packs: TaskPackCoverage[];
}

export interface TaskSizePattern {
  referenceBytes: number;
  targetBytes: number;
  delta: number;
  count: number;
  exampleIds: number[];
}

export interface TaskIdDifference {
  id: number;
  kind: "duplicate" | "target_only" | "reference_only" | "renamed";
  targetName?: string;
  referenceName?: string;
  targetBytes?: number;
  referenceBytes?: number;
}

export interface TaskIdComparisonReport {
  target: TaskSourceInfo;
  reference: TaskSourceInfo;
  targetRoots: number;
  referenceRoots: number;
  matchedIds: number;
  sameSize: number;
  grown: number;
  shrunk: number;
  renamed: number;
  targetOnly: number;
  referenceOnly: number;
  duplicateIds: number;
  differenceCount: number;
  differencesTruncated: boolean;
  patternCount: number;
  patternsTruncated: boolean;
  sizePatterns: TaskSizePattern[];
  differences: TaskIdDifference[];
}

export interface TaskFieldCandidate {
  structure: string;
  afterField: string;
  width: number;
  score: number;
  supportingSamples: number;
  testedSamples: number;
  minOffset: number;
  maxOffset: number;
  typeHints: string[];
  exampleValues: string[];
}

export interface TaskFieldCandidateReport {
  target: TaskSourceInfo;
  reference: TaskSourceInfo;
  baselineVersion: number;
  matchedIds: number;
  sampledRoots: number;
  candidateCount: number;
  candidatesTruncated: boolean;
  candidates: TaskFieldCandidate[];
}

export interface TaskLayoutOperation {
  index: number;
  kind: "insert" | "remove" | "replace";
  structure: string;
  field: string;
  afterField?: string;
  width?: number;
  fieldType?: string;
  conditions: TaskLayoutCondition[];
}

export interface TaskLayoutCondition {
  field: string;
  operator: "zero" | "non_zero" | "eq" | "not_eq" | "one_of" | "at_least" | "at_most" | "bits_any" | "bits_all";
  value?: string;
  label: string;
}

export interface TaskLayoutPatch {
  path: string;
  taskVersion: number;
  baseVersion: number;
  operations: TaskLayoutOperation[];
  verified: boolean;
  verifiedAt?: number;
  verifiedRoots?: number;
  verifiedBytes?: number;
}

/** How well one built-in layout reads a sample of an unsupported task set. */
export interface TaskLayoutProbe {
  version: number;
  sampled: number;
  exact: number;
}

/** One layout change found by aligning a newer task set with a supported one. */
export interface TaskAlignChange {
  structure: string;
  kind: "insert" | "remove" | "resize" | "array_length" | "unknown_block";
  fields: string[];
  after?: string;
  width?: number;
  delta: number;
  support: number;
  seen: number;
  /** For "array_length": the reference and the newer number of items. */
  items?: [number, number];
}

export interface TaskAlignUnresolved {
  structure: string;
  after: string;
  before: string;
  delta: number;
  reason: string;
}

/** A proposed user layout patch; `operations` go back unchanged to apply_task_alignment. */
export interface TaskAlignProposal {
  targetVersion: number;
  referenceVersion: number;
  pairs: number;
  changes: TaskAlignChange[];
  unresolved: TaskAlignUnresolved[];
  operations: unknown[];
  sampleExact: number;
  sampleTested: number;
  elapsedMs: number;
}

export interface TaskLayoutPatchReport {
  patch: TaskLayoutPatch;
  analysis: TaskAnalysisReport;
}

export interface TaskLayoutExportReport {
  path: string;
  taskVersion: number;
  baseVersion: number;
  operations: number;
}

export interface TaskSchemaField {
  name: string;
  fieldType: string;
  conditions: string[];
  patched: boolean;
  integer: boolean;
  fixedWidth?: number;
}

export interface TaskSchemaStructure {
  name: string;
  root: boolean;
  fields: TaskSchemaField[];
}

export interface TaskSchemaView {
  taskVersion: number;
  baselineVersion: number;
  source: "built_in" | "baseline" | "user_patch";
  root: string;
  structures: TaskSchemaStructure[];
}

export interface TaskTreeNode {
  id: number;
  name: string;
  /** Child indexes from the selected root. Empty means the root task. */
  path: number[];
  children: TaskTreeNode[];
}

export interface TaskFieldView {
  name: string;
  offset: number;
  size: number;
  ty: string;
  value?: string;
  interpretation?: string;
  children?: TaskFieldView[];
  raw: boolean;
  /** Stable names from the selected task to this field. */
  path: string[];
  editable: boolean;
  changed: boolean;
  reference?: TaskFieldReference;
}

export interface TaskFieldEdit {
  pack: number;
  root: number;
  taskPath: number[];
  fieldPath: string[];
  value: string;
}

export interface TaskChangedRoot {
  pack: number;
  root: number;
}

export interface TaskEditState {
  undo?: string;
  redo?: string;
  changedRoots: TaskChangedRoot[];
  /** When the task set was last saved in this session (unix ms). */
  lastSaved?: number;
}

export interface TaskCloneReport {
  state: TaskEditState;
  pack: number;
  root: number;
  path: number[];
  id: number;
  name: string;
  /** Number of quests in the copied subtree. */
  tasks: number;
}

export interface TaskMoveReport {
  state: TaskEditState;
  pack: number;
  root: number;
  path: number[];
  id: number;
  name: string;
  /** Number of quests in the moved subtree. */
  tasks: number;
}

export interface TaskDeleteReference {
  sourceId: number;
  sourceName: string;
  pack: number;
  root: number;
  path: number[];
  field: string;
  targetId: number;
}

export interface TaskDeletePreview {
  pack: number;
  root: number;
  path: number[];
  id: number;
  name: string;
  tasks: number;
  /** Deleted IDs no surviving quest has. */
  lostIds: number[];
  referenceCount: number;
  referencesTruncated: boolean;
  references: TaskDeleteReference[];
  token: string;
  /** The open elements.data, when one is open. */
  elementsPath: string | null;
  /** elements.data places naming the deleted quests. */
  elementUses: TaskIdUse[];
}

export interface TaskDeleteReport {
  state: TaskEditState;
  pack: number;
  root: number;
  path: number[];
  id: number;
  name: string;
  tasks: number;
  /** elements.data after clearing the deleted quests' IDs, when places were chosen. */
  elements: EditState | null;
  elementsError: string | null;
}

export interface TaskHistoryEntry {
  id: number;
  label: string;
  time: number;
  taskId: number;
  taskName: string;
  field: string;
  old: string;
  new: string;
  undone: boolean;
  /** When a revert from the history took this entry back (unix ms). */
  revertedAt?: number;
  /** The task set was last saved after this entry (unix ms). */
  savedAt?: number;
  /** Why this entry cannot be reverted on its own right now. */
  revertBlocked?: string;
}

export type TaskProblemKind = "duplicate_root" | "duplicate_id" | "broken_reference" | "self_reference" | "hierarchy_links" | "missing_element" | "full_pack";

export interface TaskProblem {
  kind: TaskProblemKind;
  pack: number;
  /** None for pack-level problems. */
  root?: number;
  path: number[];
  id: number;
  name: string;
  /** The field, as "fixed › premise tasks[0]". */
  field?: string;
  target?: number;
  message: string;
}

export interface TaskProblemKindSummary {
  kind: TaskProblemKind;
  severity: ProblemSeverity;
  title: string;
  description: string;
  count: number;
}

export interface TaskProblemReport {
  kinds: TaskProblemKindSummary[];
  problems: TaskProblem[];
  truncated: boolean;
  /** Item and monster IDs are checked only while an elements.data is open. */
  elementsChecked: boolean;
  tasks: number;
  elapsedMs: number;
}

export interface TaskExportTarget {
  pack: number;
  root: number;
  path: number[];
}

export interface TaskExportReport {
  path: string;
  tasks: number;
  bytes: number;
}

export interface TaskImportChange {
  sourceRow: number;
  id: number;
  name: string;
  field: string;
  old: string;
  new: string;
}

export interface TaskImportAddition {
  sourceRow: number;
  id: number;
  name: string;
  pack: number;
  /** Tasks in the added tree, the top-level task included. */
  tasks: number;
}

export interface TaskImportIssue {
  sourceRow: number;
  id?: number;
  message: string;
}

export interface TaskImportReport {
  token: string;
  sourceVersion: number;
  total: number;
  changing: number;
  adding: number;
  unchanged: number;
  rejected: number;
  fields: number;
  changes: TaskImportChange[];
  additions: TaskImportAddition[];
  issues: TaskImportIssue[];
  /** Set once the import was applied. */
  state?: TaskEditState;
}

export interface TaskCompareFile {
  path: string;
  version: number;
  roots: number;
  tasks: number;
  packs: number;
  size: number;
}

export interface TaskFieldDiff {
  /** Dotted path, as in JSON exports. */
  field: string;
  /** Value in the open file; missing when the field does not exist there. */
  this?: string;
  other?: string;
  copyable: boolean;
}

export interface TaskChangedTask {
  id: number;
  name: string;
  otherName?: string;
  pack: number;
  root: number;
  path: number[];
  moved: boolean;
  subtasksDiffer: boolean;
  fieldCount: number;
  copyableCount: number;
}

export interface TaskOneSided {
  id: number;
  name: string;
  pack: number;
  root: number;
  path: number[];
  tasks: number;
  copyable: boolean;
  reason?: string;
}

export interface TaskCompareReport {
  this: TaskCompareFile;
  other: TaskCompareFile;
  sameLayout: boolean;
  identical: number;
  changedCount: number;
  changed: TaskChangedTask[];
  onlyThisCount: number;
  onlyThis: TaskOneSided[];
  onlyOtherCount: number;
  onlyOther: TaskOneSided[];
  ambiguous: number;
  elapsedMs: number;
}

export interface TaskCopySelection {
  fields: { id: number; field: string }[];
  /** Tasks whose every copyable differing field is copied. */
  allFields: number[];
  /** Compared-only top-level tasks to add with their subquests. */
  tasks: number[];
}

export type TaskTextGroup = "names" | "descriptions" | "dialogs";

export interface TaskTranslationGroup {
  group: TaskTextGroup;
  tasks: number;
  fields: number;
  tooLong: number;
}

export interface TaskTranslationSample {
  id: number;
  name: string;
  group: TaskTextGroup;
  field: string;
  old: string;
  new: string;
}

export interface TaskTranslationIssue {
  id: number;
  name: string;
  field?: string;
  message: string;
}

export interface TaskTranslationReport {
  token: string;
  sourcePath: string;
  sourceVersion: number;
  version: number;
  matched: number;
  missingSource: number;
  ambiguous: number;
  groups: TaskTranslationGroup[];
  /** Open texts left as they are because the source text is blank. */
  blank: number;
  same: number;
  /** Talks whose windows or options differ in shape. */
  shape: number;
  tooLong: number;
  samples: TaskTranslationSample[];
  issues: TaskTranslationIssue[];
  elapsedMs: number;
}

export interface TaskSaveOptions {
  path: string;
  backup: boolean;
}

export interface TaskSavePlan {
  path: string;
  replaces: boolean;
  sameFile: boolean;
  changedOnDisk: boolean;
  readOnly: boolean;
  changedRoots: number;
  changedPacks: number;
  packCount: number;
  size: number;
  /** Saving appended top-level tasks starts a new undo history. */
  clearsHistory: boolean;
  backup?: string;
}

export interface TaskSaveReport {
  path: string;
  size: number;
  changedRoots: number;
  changedPacks: number;
  historyCleared: boolean;
  backup?: string;
}

export interface TaskFieldReference {
  kind: "task" | "element" | "skill" | "buff" | "title";
  id: number;
  label: string;
  description?: string;
  list?: number;
  row?: number;
  pack?: number;
  root?: number;
  path?: number[];
}

export interface TaskDetail {
  pack: number;
  root: number;
  path: number[];
  id: number;
  name: string;
  rootBytes: number;
  taskOffset: number;
  taskSize: number;
  tree: TaskTreeNode;
  fields: TaskFieldView[];
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

export interface TranslationChange {
  list: number;
  row: number;
  id: number;
  field: string;
  old: string;
  new: string;
}

export interface TranslationIssue {
  list: number;
  id?: number;
  field?: string;
  message: string;
}

export interface TranslationListReport {
  list: number;
  sourceList?: number;
  name: string;
  textFields: number;
  matchedRecords: number;
  changedRecords: number;
  fieldChanges: number;
  missingSource: number;
  emptySource: number;
  rejected: number;
}

export interface TranslationReport {
  token: string;
  sourcePath: string;
  sourceVersion: number;
  targetVersion: number;
  lists: TranslationListReport[];
  matchedRecords: number;
  changedRecords: number;
  fieldChanges: number;
  missingSource: number;
  emptySource: number;
  rejected: number;
  changes: TranslationChange[];
  issues: TranslationIssue[];
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

/** A row operation on a task list (`edit_task_array`). */
export type TaskArrayEdit = { kind: "add" } | { kind: "clone"; index: number } | { kind: "remove"; index: number };

/** A searchable task field (`task_search_fields`). */
export interface TaskSearchField {
  /** `success_award.candidates.items.item_id` */
  path: string;
  kind: "int" | "float" | "bool" | "text";
  /** The lists the field sits in, outermost first. */
  arrays: string[];
  reference?: "task" | "element";
  /** The same field in every award or dialog (`any:award:candidates.items.item_id`). */
  any?: string;
}

export type TaskSearchOp = "eq" | "ne" | "lt" | "le" | "gt" | "ge" | "in" | "not_in" | "contains" | "starts" | "ends" | "has_flags" | "lacks_flags" | "empty" | "not_empty";

export interface TaskSearchCondition {
  /** A field path or an `any:` field. */
  field: string;
  op: TaskSearchOp;
  value: string;
  /** Must hold on the same list row as the condition before it. */
  sameRow: boolean;
}

export type TaskSearchScope = { kind: "all" } | { kind: "top_level" } | { kind: "under"; pack: number; root: number; path: number[] };

export type TaskSearchQuery =
  | { mode: "conditions"; conditions: TaskSearchCondition[]; matchAll: boolean; scope: TaskSearchScope }
  | { mode: "value"; value: string; referencesOnly: boolean; caseSensitive: boolean; scope: TaskSearchScope };

export interface TaskSearchHit {
  pack: number;
  root: number;
  path: number[];
  id: number;
  name: string;
  matches: { field: string; value: string }[];
  more: number;
}

export interface TaskSearchResults {
  hits: TaskSearchHit[];
  total: number;
  scannedTasks: number;
  scannedRoots: number;
  truncated: boolean;
  elapsedMs: number;
}

/** One option of a talk window: `target` is a child window ID or `0x80000000 | function`. */
export interface TaskDialogOption {
  target: number;
  text: string;
  parameter: number;
}

export interface TaskDialogWindow {
  id: number;
  /** Set on reading (-1 = 4294967295 for the root); the tree decides it on writing. */
  parentId: number;
  text: string;
  options: TaskDialogOption[];
}

/** One NPC talk of a task (`task_dialogs`), root window first. */
export interface TaskDialog {
  talk: "delivery" | "unqualified" | "item_delivery" | "execution" | "award";
  prompt: string;
  windows: TaskDialogWindow[];
}

/** A quest reference an ID change rewrites. */
export interface TaskIdReference {
  pack: number;
  root: number;
  path: number[];
  taskId: number;
  taskName: string;
  field: string;
}

/** An elements.data field holding a quest ID. */
export interface TaskIdUse {
  list: number;
  listName: string;
  row: number;
  id: number;
  name: string;
  field: string;
  off: number;
  /** The quest ID it holds. */
  taskId: number;
}

export interface TaskIdChangePreview {
  oldId: number;
  newId: number;
  name: string;
  references: TaskIdReference[];
  /** Other quests with the same old ID; their references stay. */
  duplicates: number;
  roots: number;
  elementsPath: string | null;
  elementUses: TaskIdUse[];
}

export interface TaskIdChangeResult {
  tasks: TaskEditState;
  elements: EditState | null;
  elementsError: string | null;
}

// ── dyn_tasks.data (dynamic tasks the server sends to clients) ──

export type DynAwardLayout = "classic" | "shifted" | "unknown";

export interface DynVert { x: number; y: number; z: number }
export interface DynTaskTime { year: number; month: number; day: number; hour: number; minute: number; weekday: number }

/** The client's packed ITEM_WANTED (31 bytes). */
export interface DynItem {
  itemId: number;
  commonItem: number;
  amount: number;
  probability: number;
  bound: number;
  period: number;
  timetable: number;
  dayOfWeek: number;
  hour: number;
  minute: number;
  refineCondition: number;
  refineLevel: number;
  replacementItemId: number;
}

export interface DynMonster {
  monsterId: number;
  amount: number;
  dropItemId: number;
  dropItemAmount: number;
  dropCommonItem: number;
  dropProbability: number;
  killerLevel: number;
}

export interface DynZone { flag: number; world: number; min: DynVert; max: DynVert }
export interface DynTransfer { flag: number; world: number; point: DynVert }
export interface DynGivenItems { commonCount: number; taskCount: number; items: DynItem[] }
export interface DynTimetableEntry { kind: number; start: DynTaskTime; end: DynTaskTime }

/** `method` decides which of the other fields are stored. */
export interface DynGoal {
  method: number;
  monsters: DynMonster[];
  items: DynItem[];
  gold: number;
  siteId: number;
  siteMin: DynVert;
  siteMax: DynVert;
  wait: number;
}

export interface DynCandidate { random: number; items: DynItem[] }

export interface DynAward {
  gold: number | null;
  experience: number | null;
  sp: number | null;
  reputation: number | null;
  candidates: DynCandidate[] | null;
  extraMask: number;
}

export interface DynOption { id: number; param: number; text: string }
export interface DynWindow { id: number; parent: number; text: string; options: DynOption[] }
export interface DynTalk { prompt: string; windows: DynWindow[] }

export interface DynTask {
  dynType: number;
  specialAward: number;
  id: number;
  name: string;
  flags: number[];
  levelMin: number;
  levelMax: number;
  timeLimit: number | null;
  reputation: number | null;
  period: number | null;
  premiseItems: DynItem[] | null;
  zone: DynZone | null;
  transfer: DynTransfer | null;
  givenItems: DynGivenItems | null;
  deposit: number | null;
  premiseTasks: number[] | null;
  gender: number | null;
  occupations: number[] | null;
  mutexTasks: number[] | null;
  timetable: DynTimetableEntry[] | null;
  goal: DynGoal;
  finishType: number;
  award: DynAward;
  description: string;
  okText: string;
  noText: string;
  talks: DynTalk[];
  subtasks: DynTask[];
  extraMask: number;
  mask2: number;
}

export interface DynRow {
  index: number;
  uid: number;
  id: number;
  name: string;
  dynType: number;
  specialAward: number;
  method: number;
  subtasks: number;
  status: "" | "changed" | "added";
}

export interface DynHistoryEntry {
  id: number;
  label: string;
  time: number;
  taskId: number;
  taskName: string;
  undone: boolean;
}

export interface DynView {
  path: string;
  size: number;
  timeMark: number;
  version: number;
  layout: DynAwardLayout;
  rows: DynRow[];
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  history: DynHistoryEntry[];
  savedEntries: number | null;
  lastSaved: number | null;
}

export interface DynProblem {
  severity: "error" | "warning";
  index: number;
  taskId: number;
  taskName: string;
  message: string;
}

export interface DynProblemReport {
  problems: DynProblem[];
  tasksChecked: boolean;
  elementsChecked: boolean;
}

export interface DynLabels {
  elements: Record<string, string>;
  tasks: Record<string, string>;
}

export interface DynSaveReport {
  path: string;
  size: number;
  tasks: number;
  timeMark: number;
  backup: string | null;
}

/** One task in the dyn_tasks.data rewards overview. */
export interface DynOverviewRow {
  index: number;
  uid: number;
  id: number;
  name: string;
  dynType: number;
  specialAward: number;
  levelMin: number;
  levelMax: number;
  method: number;
  gold: number | null;
  experience: number | null;
  sp: number | null;
  reputation: number | null;
  /** Item groups: [random, [[item, amount], …]]. */
  groups: [boolean, [number, number][]][];
  status: "" | "changed" | "added";
}

export interface DynCompareRow {
  id: number;
  name: string;
  specialAward: number;
  status: "missing" | "different" | "same" | "only_here";
  /** Top-level parts that differ (DynTask keys). */
  fields: string[];
  blocked: string | null;
}

export interface DynComparison {
  path: string;
  tasks: number;
  timeMark: number;
  layout: DynAwardLayout;
  rows: DynCompareRow[];
}

// ── task_npc.data (where the quest tracker finds NPCs and monsters) ──

export interface TaskNpcRow {
  id: number;
  /** Map (instance) ID; 0 = no location. */
  map: number;
  x: number;
  y: number;
  z: number;
  /** Padding bytes, kept as they were. */
  pad: number;
}

export interface TaskNpcFile {
  path: string;
  size: number;
  timeMark: number;
  token: string;
  rows: TaskNpcRow[];
}

export interface TaskNpcSaveRequest {
  openedPath: string;
  targetPath: string;
  token: string;
  rows: TaskNpcRow[];
  backup: boolean;
  replaceChanged: boolean;
}

export interface TaskNpcSaveReport {
  path: string;
  size: number;
  rows: number;
  timeMark: number;
  token: string;
  backup: string | null;
}

// ── npcgen.data (one server map's spawns) ──

export interface NpcGenVec3 { x: number; y: number; z: number }

export interface NpcGenGenerator {
  id: number;
  count: number;
  refresh: number;
  diedTimes: number;
  aggressive: number;
  offsetWater: number;
  offsetTerrain: number;
  faction: number;
  factionHelper: number;
  factionAccept: number;
  needHelp: number;
  defaultFaction: number;
  defaultFactionHelper: number;
  defaultFactionAccept: number;
  pathId: number;
  loopType: number;
  speedFlag: number;
  deadTime: number;
}

export interface NpcGenArea {
  kind: number;
  position: NpcGenVec3;
  direction: NpcGenVec3;
  extents: NpcGenVec3;
  npcType: number;
  groupType: number;
  initGen: number;
  revive: number;
  validOnce: number;
  genId: number;
  controller: number;
  lifeTime: number;
  maxCount: number;
  exportId: number;
  attachNum: number;
  attached: number[];
  phase: number;
  generators: NpcGenGenerator[];
}

export interface NpcGenResource { kind: number; template: number; refresh: number; count: number; heightOffset: number }

export interface NpcGenResourceArea {
  position: NpcGenVec3;
  extentX: number;
  extentZ: number;
  initGen: number;
  autoRevive: number;
  validOnce: number;
  genId: number;
  direction: [number, number];
  radius: number;
  controller: number;
  maxCount: number;
  exportId: number;
  attachNum: number;
  attached: number[];
  phase: number;
  resources: NpcGenResource[];
}

export interface NpcGenObject { id: number; position: NpcGenVec3; direction: [number, number]; radius: number; scale: number; controller: number; phase: number }

export interface NpcGenTime { year: number; month: number; week: number; day: number; hours: number; minutes: number }

export interface NpcGenController {
  id: number;
  controllerId: number;
  name: string;
  nameRaw: number[];
  active: number;
  waitTime: number;
  stopTime: number;
  activeTimeInvalid: number;
  stopTimeInvalid: number;
  activeTime: NpcGenTime;
  stopTimeAt: NpcGenTime;
  activeTimeRange: number;
  repeat: number;
  segmentLogic: number;
  segments: [NpcGenTime, NpcGenTime][];
}

export type NpcGenSection = "areas" | "resources" | "objects" | "controllers";
export type NpcGenItem =
  | { section: "areas"; item: NpcGenArea }
  | { section: "resources"; item: NpcGenResourceArea }
  | { section: "objects"; item: NpcGenObject }
  | { section: "controllers"; item: NpcGenController };

/** The character in a running game client: position and (unit) facing direction. */
export interface GamePosition {
  x: number;
  y: number;
  z: number;
  direction: { x: number; y: number; z: number } | null;
}

/** How a nearby thing was classified (elements.data decides NPC/monster and mine/item). */
export type NearbyClass = "npc" | "monster" | "unknown" | "mine" | "dynamic" | "item";

/** Something the running game client has loaded. */
export interface NearbyRow {
  kind: "npc" | "matter" | "dynamic";
  runtimeId: number;
  template: number;
  position: { x: number; y: number; z: number };
  direction: { x: number; y: number; z: number } | null;
  rotation: [number, number, number] | null;
  dropper: number;
  /** Null while this client build's phase offset is unknown. */
  phase: number | null;
  class: NearbyClass;
  label: string | null;
}

export interface NearbyFetch {
  player: GamePosition;
  rows: NearbyRow[];
}

/** One row to add to npcgen.data. */
export interface NearbyImport {
  kind: "npc" | "monster" | "mine" | "dynamic";
  template: number;
  position: { x: number; y: number; z: number };
  direction: { x: number; y: number; z: number } | null;
  rotation: [number, number, number] | null;
  phase: number | null;
}

/** A running elementclient.exe. */
export interface RunningClient {
  pid: number;
  path: string | null;
}

/** A map of the configured client (configs.pck instance.txt). */
export interface ClientMap {
  id: number;
  name: string;
  /** Names the client's map images (Surfaces/MidMaps/<path>.dds). */
  path: string;
  /** Names the server's map folder. */
  dataPath: string;
  rows: number;
  cols: number;
  hasImage: boolean;
}

export interface NpcGenSummary {
  index: number;
  x: number;
  z: number;
  extX: number;
  extZ: number;
  kind: number;
  ids: number[];
  count: number;
  controller: number;
  label: string;
  changed: boolean;
}

export interface NpcGenView {
  path: string;
  version: number;
  size: number;
  areas: NpcGenSummary[];
  resources: NpcGenSummary[];
  objects: NpcGenSummary[];
  controllers: NpcGenSummary[];
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  history: { id: number; label: string; time: number; undone: boolean }[];
  savedEntries: number | null;
}

export interface NpcGenSaveReport { path: string; size: number; backup: string | null }
