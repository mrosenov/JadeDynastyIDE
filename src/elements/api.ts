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
  TalkTextEdit,
  FileSummary,
  ImportCandidate,
  LayoutAnalysis,
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
  TranslationReport,
  PathDataFile,
  PathDataRow,
  PathDataSaveReport,
  PathDataSaveRequest,
  PathDataJsonImport,
  PathDataJsonReport,
  TasksFileSummary,
  TaskDetail,
  TaskSearchReport,
  TaskFieldEdit,
  TaskEditState,
  TaskCloneReport,
  TaskMoveReport,
  TaskDeletePreview,
  TaskDeleteReport,
  TaskHistoryEntry,
  TaskSaveOptions,
  TaskSavePlan,
  TaskSaveReport,
  TaskSourceInfo,
  TaskSourceVersion,
  TaskAnalysisReport,
  TaskIdComparisonReport,
  TaskFieldCandidateReport,
  TaskLayoutPatch,
  TaskLayoutPatchReport,
  TaskLayoutExportReport,
  TaskLayoutCondition,
  TaskSchemaView,
  TaskProblemReport,
  TaskExportTarget,
  TaskExportReport,
  TaskImportReport,
  TaskCompareReport,
  TaskFieldDiff,
  TaskCopySelection,
  TaskTextGroup,
  TaskTranslationReport,
  TaskDeleteReference, TaskArrayEdit, TaskDialog, TaskSearchField, TaskSearchQuery, TaskSearchResults } from "./types";

export const openElements = (path: string) => invoke<FileSummary>("open_elements", { path });

/** Strictly reads a standalone client path.data table. */
export const openPathData = (path: string) => invoke<PathDataFile>("open_path_data", { path });
/** Validates and atomically writes a complete path.data table. */
export const savePathData = (request: PathDataSaveRequest) => invoke<PathDataSaveReport>("save_path_data", { request });
/** Writes a complete, versioned JSON representation of a path table. */
export const exportPathDataJson = (path: string, sourcePath: string, rows: PathDataRow[]) => invoke<PathDataJsonReport>("export_path_data_json", { path, sourcePath, rows });
/** Reads and validates a versioned path export or a plain array of path rows. */
export const importPathDataJson = (path: string) => invoke<PathDataJsonImport>("import_path_data_json", { path });

/** Opens and verifies a static tasks.data index and every numbered pack. */
export const openTasks = (path: string) => invoke<TasksFileSummary>("open_tasks", { path });
export const taskSourceVersion = (path: string) => invoke<TaskSourceVersion>("task_source_version", { path });
export const inspectTasks = (path: string) => invoke<TaskSourceInfo>("inspect_tasks", { path });
export const analyzeTasks = (path: string, baselineVersion: number) => invoke<TaskAnalysisReport>("analyze_tasks", { path, baselineVersion });
export const compareTaskIds = (path: string, referencePath: string) => invoke<TaskIdComparisonReport>("compare_task_ids", { path, referencePath });
/** Scores read-only fixed-width insertions after normalizing any accepted user-layout fields. */
export const scoreTaskFields = (path: string, referencePath: string) => invoke<TaskFieldCandidateReport>("score_task_fields", { path, referencePath });
export const getTaskLayoutPatch = (version: number) => invoke<TaskLayoutPatch | null>("task_layout_patch", { version });
export const getTaskSchema = (version: number, baselineVersion: number) => invoke<TaskSchemaView>("task_schema", { version, baselineVersion });
export const analyzeTaskLayoutPatch = (path: string) => invoke<TaskLayoutPatchReport>("analyze_task_layout_patch", { path });
/** Accepts a user layout only after every root decodes and re-encodes byte-for-byte. */
export const verifyTaskLayout = (path: string, baselineVersion: number) => invoke<TaskLayoutPatch>("verify_task_layout", { path, baselineVersion });
/** Returns an accepted user layout to read-only analysis before schema changes. */
export const editTaskLayout = (version: number) => invoke<TaskLayoutPatch | null>("edit_task_layout", { version });
export const addTaskLayoutField = (path: string, baseVersion: number, structure: string, afterField: string, name: string, width: number, fieldType: string) =>
  invoke<TaskLayoutPatchReport>("add_task_layout_field", { path, baseVersion, structure, afterField, name, width, fieldType });
export const addTaskLayoutCountedArray = (path: string, baseVersion: number, structure: string, afterField: string, name: string, countField: string, itemType: string) =>
  invoke<TaskLayoutPatchReport>("add_task_layout_counted_array", { path, baseVersion, structure, afterField, name, countField, itemType });
export const removeTaskLayoutField = (path: string, baseVersion: number, structure: string, field: string) =>
  invoke<TaskLayoutPatchReport>("remove_task_layout_field", { path, baseVersion, structure, field });
export const replaceTaskLayoutFieldType = (path: string, baseVersion: number, structure: string, field: string, fieldType: string) =>
  invoke<TaskLayoutPatchReport>("replace_task_layout_field_type", { path, baseVersion, structure, field, fieldType });
export const setTaskLayoutOperationType = (path: string, index: number, fieldType: string) =>
  invoke<TaskLayoutPatchReport>("set_task_layout_operation_type", { path, index, fieldType });
export const setTaskLayoutOperationConditions = (path: string, index: number, conditions: TaskLayoutCondition[]) =>
  invoke<TaskLayoutPatchReport>("set_task_layout_operation_conditions", { path, index, conditions: conditions.map(({ field, operator, value }) => ({ field, operator, value })) });
export const exportTaskLayoutPatch = (version: number, targetPath: string) => invoke<TaskLayoutExportReport>("export_task_layout_patch", { version, targetPath });
export const importTaskLayoutPatch = (tasksPath: string, patchPath: string) => invoke<TaskLayoutPatchReport>("import_task_layout_patch", { tasksPath, patchPath });
export const removeTaskLayoutOperation = (path: string, index: number) => invoke<TaskLayoutPatchReport>("remove_task_layout_operation", { path, index });
/** Decodes one root lazily, then selects a task in its recursive hierarchy. */
export const getTask = (pack: number, root: number, path: number[]) => invoke<TaskDetail>("get_task", { pack, root, path });
/** Searches root and nested quests through the background task index. */
export const searchTasks = (query: string, limit = 50_000) => invoke<TaskSearchReport>("search_tasks", { query, limit });
/** Changes one safe leaf inside a task root in memory. */
export const editTaskField = (edit: TaskFieldEdit) => invoke<TaskEditState>("edit_task_field", { edit });
/** Sets several values of one task as one undo step. */
export const editTaskFields = (pack: number, root: number, taskPath: number[], values: { fieldPath: string[]; value: string }[], label: string) =>
  invoke<TaskEditState>("edit_task_fields", { pack, root, taskPath, values, label });
/** Adds, clones or removes a row of a list of the task; `countPath` names the count of a fixed list. */
export const editTaskArray = (pack: number, root: number, taskPath: number[], arrayPath: string[], countPath: string[] | null, edit: TaskArrayEdit) =>
  invoke<TaskEditState>("edit_task_array", { pack, root, taskPath, arrayPath, countPath, edit });
/** The talks of a task as trees of windows and options. */
export const taskDialogs = (pack: number, root: number, taskPath: number[]) => invoke<TaskDialog[]>("task_dialogs", { pack, root, taskPath });
/** Replaces one talk of a task (one undo step). */
export const setTaskDialog = (pack: number, root: number, taskPath: number[], dialog: TaskDialog, label: string) => invoke<TaskEditState>("set_task_dialog", { pack, root, taskPath, dialog, label });
/** The searchable fields of the open task layout. */
export const taskSearchFields = () => invoke<TaskSearchField[]>("task_search_fields");
/** Advanced task search over the whole set, unsaved edits included. */
export const searchTasksAdvanced = (query: TaskSearchQuery) => invoke<TaskSearchResults>("search_tasks_advanced", { query });
/** Stops the running advanced task search. */
export const cancelTaskSearch = () => invoke<void>("cancel_task_search");
/** Roots scanned so far by the running advanced task search. */
export const taskSearchProgress = () => invoke<{ scanned: number; total: number }>("task_search_progress");
/** Character classes (ID, name) of the open elements.data. */
export const characterClasses = () => invoke<[number, string][]>("character_classes");
/** Clones a selected subquest and its descendants beside the source with fresh task IDs. */
export const cloneTaskSubtree = (pack: number, root: number, path: number[]) => invoke<TaskCloneReport>("clone_task_subtree", { pack, root, path });
/** Clones a complete top-level task tree into the same task pack with fresh task IDs. */
export const cloneTaskRoot = (pack: number, root: number) => invoke<TaskCloneReport>("clone_task_root", { pack, root });
/** Moves a selected subquest tree below another existing quest without changing task IDs. */
export const moveTaskSubtree = (source: { pack: number; root: number; path: number[]; id: number }, destination: { pack: number; root: number; path: number[]; id: number }) =>
  invoke<TaskMoveReport>("move_task_subtree", {
    sourcePack: source.pack,
    sourceRoot: source.root,
    sourcePath: source.path,
    sourceId: source.id,
    destinationPack: destination.pack,
    destinationRoot: destination.root,
    destinationPath: destination.path,
    destinationId: destination.id,
  });
/** The current root list and counts, including appended or removed top-level tasks. */
export const getTaskSummary = () => invoke<TasksFileSummary>("task_summary");
/** Finds surviving task references before removing a selected subquest tree. */
export const previewDeleteTaskSubtree = (pack: number, root: number, path: number[]) => invoke<TaskDeletePreview>("preview_delete_task_subtree", { pack, root, path });
/** Removes a previously previewed subquest tree as one undoable root edit. */
export const deleteTaskSubtree = (preview: TaskDeletePreview) => invoke<TaskDeleteReport>("delete_task_subtree", {
  pack: preview.pack,
  root: preview.root,
  path: preview.path,
  token: preview.token,
  allowReferenced: preview.referenceCount > 0,
});
export const getTaskEditState = () => invoke<TaskEditState>("task_edit_state");
export const getTaskEditHistory = () => invoke<TaskHistoryEntry[]>("task_edit_history");
export const undoTaskEdit = () => invoke<TaskEditState>("undo_task_edit");
/** Scans the complete task index for duplicate IDs, broken references, stale links and full packs. */
export const getTaskProblems = () => invoke<TaskProblemReport>("task_problems");
/** Opens another task set read-only and compares it with the open one by task ID. */
export const openTaskCompare = (path: string) => invoke<TaskCompareReport>("open_task_compare", { path });
export const taskCompare = () => invoke<TaskCompareReport>("task_compare");
/** The differing fields of one paired task. */
export const taskCompareFields = (id: number) => invoke<TaskFieldDiff[]>("task_compare_fields", { id });
export const closeTaskCompare = () => invoke<void>("close_task_compare");
/** Copies fields and whole top-level tasks from the compared set as one undo step. */
export const copyComparedTasks = (selection: TaskCopySelection) => invoke<TaskImportReport>("copy_compared_tasks", { selection });
/** Plans copying names, descriptions and dialog text from a translated tasks.data by task ID. */
export const previewTaskTranslation = (path: string) => invoke<TaskTranslationReport>("preview_task_translation", { path });
/** Applies the previewed translation of the chosen text groups as one undo step. */
export const applyTaskTranslation = (token: string, groups: TaskTextGroup[]) => invoke<TaskEditState>("apply_task_translation", { token, groups });
export const closeTaskTranslation = () => invoke<void>("close_task_translation");
/** Writes tasks (optionally with every subtask) as versioned JSON. */
export const exportTasksJson = (targets: TaskExportTarget[], subtrees: boolean, path: string) => invoke<TaskExportReport>("export_tasks_json", { targets, subtrees, path });
/** Previews a task JSON import, or applies it when the preview token is passed. */
export const importTasksJson = (path: string, token?: string) => invoke<TaskImportReport>("import_tasks_json", { path, token });
/** Tasks whose fields name the given task ID. */
export const getTaskReferencedBy = (id: number) => invoke<TaskDeleteReference[]>("task_referenced_by", { id });
/** Takes back one applied task edit from the history without undoing later ones. */
export const revertTaskEntry = (id: number) => invoke<TaskEditState>("revert_task_entry", { id });
export const redoTaskEdit = () => invoke<TaskEditState>("redo_task_edit");
export const revertTaskEdits = () => invoke<TaskEditState>("revert_task_edits");
export const taskSavePlan = (options: TaskSaveOptions) => invoke<TaskSavePlan>("task_save_plan", { options });
export const saveTasks = (options: TaskSaveOptions) => invoke<TaskSaveReport>("save_tasks", { options });

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

/** Previews UTF-16 text matched by list, record ID and field path. */
export const previewTranslation = (path: string) => invoke<TranslationReport>("preview_translation", { path });
/** Applies selected lists from an unchanged translation preview. */
export const applyTranslation = (path: string, token: string, lists: number[]) => invoke<EditState>("apply_translation", { path, token, lists });

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
export const revertTalk = (index: number, label: string) => invoke<EditState>("revert_talk", { index, label });
export const editHistory = () => invoke<HistoryEntry[]>("edit_history");
/** Takes back one edit; fails with "CONFLICT: …" when later edits changed the same fields (unless forced). */
export const revertHistoryEntry = (id: number, force: boolean) => invoke<EditState>("revert_history_entry", { id, force });
export const getEditState = () => invoke<EditState>("edit_state");

export const listTalks = () => invoke<TalkSummary[]>("list_talks");
export const getTalk = (index: number) => invoke<TalkDetail>("get_talk", { index });
/** Changes only human-facing dialog strings, as one undo step. */
export const editTalkText = (index: number, edit: TalkTextEdit) => invoke<EditState>("edit_talk_text", { index, edit });

export const getRecord = (list: number, index: number) => invoke<RecordDetail>("get_record", { list, index });

export const schemaContext = () => invoke<SchemaContext>("schema_context");

export const getListSchema = (list: number) => invoke<ListSchema>("get_list_schema", { list });

export const previewListSchema = (list: number, index: number, def: ListDef) =>
  invoke<RecordDetail>("preview_list_schema", { list, index, def });

export const saveListSchema = (list: number, def: ListDef) => invoke<FileSummary>("save_list_schema", { list, def });

export const resetListSchema = (list: number) => invoke<FileSummary>("reset_list_schema", { list });

export const importCandidates = (list: number) => invoke<ImportCandidate[]>("import_candidates", { list });

/** Builds an AI schema proposal from a trusted reference file. Nothing is saved by this command. */
export const analyzeListLayout = (referencePath: string, sourceLayout: string, list: number) => invoke<LayoutAnalysis>("analyze_list_layout", { referencePath, sourceLayout, list });

/** Locally aligns an older exact schema from matching record bytes. */
export const analyzeListFromReference = (referencePath: string, sourceLayout: string, list: number) =>
  invoke<LayoutAnalysis>("analyze_list_from_reference", { referencePath, sourceLayout, list });

export const getSettings = () => invoke<SettingsView>("get_settings");

export const saveSettings = (settings: Settings) => invoke<SettingsView>("save_settings", { settings });

export const inspectClient = (dir: string) => invoke<ClientInfo>("inspect_client", { dir });

/** URL of an item icon served by the app (by path.data ID). No slashes: convertFileSrc encodes them. */
export const iconUrl = (generation: number, pathId: number) => convertFileSrc(`${generation}-${pathId}`, "jdicon");

/** URL of a standalone client image served by the app (by path.data ID). */
export const imageUrl = (generation: number, pathId: number) => convertFileSrc(`${generation}-${pathId}`, "jdimage");

export const namedSets = () => invoke<SetSummary[]>("named_sets");

export const namedSet = (key: string) => invoke<SetDetail>("named_set", { key });

export const saveNamedSet = (kind: SetKind, set: NamedSet) => invoke<SetsChanged>("save_named_set", { kind, set });

/** Deletes a set (a built-in one is hidden until restored). */
export const deleteNamedSet = (key: string) => invoke<SetsChanged>("delete_named_set", { key });
/** Drops the user's version of a set: reverts an edited built-in, restores a deleted one. */
export const revertNamedSet = (key: string) => invoke<SetsChanged>("revert_named_set", { key });

export const referencedBy = (list: number, row: number) => invoke<ReferencedBy>("referenced_by", { list, row });
