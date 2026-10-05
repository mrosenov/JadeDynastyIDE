import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type {
  FindResult,
  BulkEdit,
  BulkReport,
  HistoryEntry,
  EditState,
  FieldEdit,
  CompareSummary,
  CompareCopyRequest,
  CoverageRow,
  ExportSource,
  Exported,
  ImportReport,
  ListDiff,
  ProblemReport,
  SearchFieldName,
  SearchQuery,
  SearchReport,
  TalkDetail,
  TalkSummary,
  FileSummary,
  ImportCandidate,
  ListDef,
  ListSchema,
  RecordDetail,
  RecordRow,
  SchemaContext,
  ClientInfo,
  Settings,
  SettingsView,
  NamedSet,
  SetDetail,
  SetKind,
  SetSummary,
  SetsChanged,
  ReferencedBy,
  SaveOptions,
  SavePlan,
  Saved,
  PickerRequest,
  PickerResult,
} from "./types";

export const openElements = (path: string) => invoke<FileSummary>("open_elements", { path });

export const listRecords = (list: number) => invoke<RecordRow[]>("list_records", { list });

/** Records of every list by ID or name. */
export const findRecords = (query: string) => invoke<FindResult>("find_records", { query });

/** Bounded, on-demand choices for a reference or client resource field. */
export const searchPicker = (request: PickerRequest) => invoke<PickerResult>("picker_search", { request });

export const searchRecords = (query: SearchQuery) => invoke<SearchReport>("search_records", { query });
export const searchFieldNames = (list: number | null) => invoke<SearchFieldName[]>("search_field_names", { list });

/** Scans the whole file for problems (see the Problems panel). */
export const listProblems = () => invoke<ProblemReport>("list_problems");

export const layoutCoverage = () => invoke<CoverageRow[]>("layout_coverage");

export const exportRecords = (source: ExportSource, labels: boolean, path: string) =>
  invoke<Exported>("export_records", { source, labels, path });

/** Without a token, previews; with the preview token, applies that exact import. */
export const importRecords = (path: string, token?: string) => invoke<ImportReport>("import_records", { path, token });

/** Opens a second file to compare the open one with ("this" vs "other"). */
export const openCompare = (path: string) => invoke<CompareSummary>("open_compare", { path });
export const compareSummary = () => invoke<CompareSummary>("compare_summary");
export const compareList = (pair: { this: number | null; other: number | null }) =>
  invoke<ListDiff>("compare_list", { this: pair.this, other: pair.other });
export const compareMarkdown = (otherIsOlder: boolean) => invoke<string>("compare_markdown", { otherIsOlder });
export const copyCompare = (request: CompareCopyRequest) => invoke<EditState>("copy_compare", { request });
export const closeCompare = () => invoke<void>("close_compare");

/** Sets fields of a record (one undo step). */
export const editRecord = (list: number, row: number, edits: FieldEdit[], label: string) =>
  invoke<EditState>("edit_record", { list, row, edits, label });
/** Copies a record to the end of its list with a new ID from its ID space. */
export const cloneRecord = (list: number, row: number) => invoke<EditState>("clone_record", { list, row });
export const deleteRecord = (list: number, row: number) => invoke<EditState>("delete_record", { list, row });
export const fileSummary = () => invoke<FileSummary>("file_summary");
/** Plans a bulk edit over search results, or with `apply` makes it (one undo step). */
export const bulkEdit = (edit: BulkEdit, apply: boolean) => invoke<BulkReport>("bulk_edit", { edit, apply });
export const savePlan = (options: SaveOptions) => invoke<SavePlan>("save_plan", { options });
export const saveElements = (options: SaveOptions) => invoke<Saved>("save_elements", { options });

export const undoEdit = () => invoke<EditState>("undo_edit");
export const redoEdit = () => invoke<EditState>("redo_edit");
/** Puts records (all changed ones without any) back as the file was opened. */
export const revertEdits = (records: [number, number][] | null, label: string) => invoke<EditState>("revert_edits", { records, label });
export const editHistory = () => invoke<HistoryEntry[]>("edit_history");
/** Takes back one edit; fails with "CONFLICT: …" when later edits changed the same fields (unless forced). */
export const revertHistoryEntry = (id: number, force: boolean) => invoke<EditState>("revert_history_entry", { id, force });
export const getEditState = () => invoke<EditState>("edit_state");

export const listTalks = () => invoke<TalkSummary[]>("list_talks");
export const getTalk = (index: number) => invoke<TalkDetail>("get_talk", { index });

export const getRecord = (list: number, index: number) => invoke<RecordDetail>("get_record", { list, index });

export const schemaContext = () => invoke<SchemaContext>("schema_context");

export const getListSchema = (list: number) => invoke<ListSchema>("get_list_schema", { list });

export const previewListSchema = (list: number, index: number, def: ListDef) =>
  invoke<RecordDetail>("preview_list_schema", { list, index, def });

export const saveListSchema = (list: number, def: ListDef) => invoke<FileSummary>("save_list_schema", { list, def });

export const resetListSchema = (list: number) => invoke<FileSummary>("reset_list_schema", { list });

export const importCandidates = (list: number) => invoke<ImportCandidate[]>("import_candidates", { list });

export const getSettings = () => invoke<SettingsView>("get_settings");

export const saveSettings = (settings: Settings) => invoke<SettingsView>("save_settings", { settings });

export const inspectClient = (dir: string) => invoke<ClientInfo>("inspect_client", { dir });

/** URL of an item icon served by the app (by path.data ID). No slashes: convertFileSrc encodes them. */
export const iconUrl = (generation: number, pathId: number) => convertFileSrc(`${generation}-${pathId}`, "jdicon");

export const namedSets = () => invoke<SetSummary[]>("named_sets");

export const namedSet = (key: string) => invoke<SetDetail>("named_set", { key });

export const saveNamedSet = (kind: SetKind, set: NamedSet) => invoke<SetsChanged>("save_named_set", { kind, set });

/** Deletes a set (a built-in one is hidden until restored). */
export const deleteNamedSet = (key: string) => invoke<SetsChanged>("delete_named_set", { key });
/** Drops the user's version of a set: reverts an edited built-in, restores a deleted one. */
export const revertNamedSet = (key: string) => invoke<SetsChanged>("revert_named_set", { key });

export const referencedBy = (list: number, row: number) => invoke<ReferencedBy>("referenced_by", { list, row });
