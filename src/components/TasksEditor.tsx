import { forwardRef, useCallback, useDeferredValue, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, ArrowRight, BarChart3, Braces, Check, Copy, Download, FileCheck2, FlaskConical, FolderOpen, GitBranch, History, Link2, ListTree, Loader2, Minus, Pencil, Plus, Redo2, RotateCcw, Search, ShieldCheck, Trash2, Undo2, Upload, X } from "lucide-react";
import { addTaskLayoutCountedArray, addTaskLayoutField, analyzeTaskLayoutPatch, analyzeTasks, cloneTaskRoot, cloneTaskSubtree, compareTaskIds, deleteTaskSubtree, editTaskField, editTaskLayout, exportTaskLayoutPatch, getTask, getTaskEditHistory, getTaskEditState, getTaskLayoutPatch, importTaskLayoutPatch, inspectTasks, moveTaskSubtree, openTasks, previewDeleteTaskSubtree, redoTaskEdit, removeTaskLayoutField, removeTaskLayoutOperation, replaceTaskLayoutFieldType, revertTaskEdits, scoreTaskFields, searchTasks, setTaskLayoutOperationConditions, setTaskLayoutOperationType, taskSourceVersion, undoTaskEdit, verifyTaskLayout } from "../elements/api";
import { bytes, count } from "../elements/format";
import type { TaskAnalysisReport, TaskDeletePreview, TaskDetail, TaskEditState, TaskFieldCandidate, TaskFieldCandidateReport, TaskFieldReference, TaskFieldView, TaskHistoryEntry, TaskIdComparisonReport, TaskLayoutCondition, TaskLayoutPatch, TaskRootSummary, TaskSearchEntry, TaskSearchReport, TaskSourceInfo, TasksFileSummary, TaskTreeNode } from "../elements/types";
import { ResourceHint } from "./FieldTree";
import { TaskSaveDialog } from "./TaskSaveDialog";
import { TaskSchemaDialog } from "./TaskSchemaDialog";
import { TaskCountedArrayDialog, type TaskCountedArrayDraft } from "./TaskCountedArrayDialog";
import { TaskBaselineFieldDialog, type TaskBaselineFieldDraft } from "./TaskBaselineFieldDialog";
import { TaskDeleteDialog } from "./TaskDeleteDialog";
import { TaskMoveDialog } from "./TaskMoveDialog";

export interface TasksEditorState {
  loaded: boolean;
  path: string | null;
  summary: TasksFileSummary | null;
  unsupported: TaskSourceInfo | null;
  analysis: TaskAnalysisReport | null;
  comparison: TaskIdComparisonReport | null;
  fieldCandidates: TaskFieldCandidateReport | null;
  layoutPatch: TaskLayoutPatch | null;
  referencePath: string | null;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  edits: TaskEditState;
  selection: { pack: number; root: number; path: number[] } | null;
}

export interface TasksEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
  undo: () => void;
  redo: () => void;
  revertAll: () => void;
  save: () => void;
}

interface Props {
  active: boolean;
  defaultPath?: string | null;
  initialState?: TasksEditorState;
  onStateChange: (state: TasksEditorState) => void;
  onOpenElement?: (list: number, row: number) => void;
}

const PAGE_SIZE = 200;
const pathKey = (path: number[]) => path.join(".");
const rootKey = (root: TaskRootSummary) => `${root.pack}:${root.root}`;
const branchKey = (root: TaskRootSummary, path: number[]) => `${rootKey(root)}:${pathKey(path)}`;
const label = (name: string) => name.replace(/^unknown_/, "unknown · ").replaceAll("_", " ");

function fixedTypeOptions(width: number) {
  switch (width) {
    case 1: return ["bool8", "uint8", "int8", "bytes[1]", "raw8"];
    case 2: return ["uint16", "int16", "bytes[2]", "raw16"];
    case 4: return ["uint32", "int32", "float32", "bytes[4]", "raw32"];
    case 8: return ["uint64", "int64", "float64", "bytes[8]", "raw64"];
    default: return [`bytes[${width}]`, `raw[${width}]`];
  }
}

function safestCandidateType(candidate: TaskFieldCandidate) {
  return [...candidate.typeHints].reverse().find((type) => type.startsWith("raw")) ?? candidate.typeHints.at(-1) ?? fixedTypeOptions(candidate.width).at(-1)!;
}

const CONDITION_OPERATORS: Array<{ value: TaskLayoutCondition["operator"]; label: string; needsValue: boolean }> = [
  { value: "non_zero", label: "is not zero", needsValue: false },
  { value: "zero", label: "is zero", needsValue: false },
  { value: "eq", label: "equals", needsValue: true },
  { value: "not_eq", label: "does not equal", needsValue: true },
  { value: "one_of", label: "is one of", needsValue: true },
  { value: "at_least", label: "is at least", needsValue: true },
  { value: "at_most", label: "is at most", needsValue: true },
  { value: "bits_any", label: "has any bits", needsValue: true },
  { value: "bits_all", label: "has all bits", needsValue: true },
];

const conditionNeedsValue = (operator: TaskLayoutCondition["operator"]) => CONDITION_OPERATORS.find((candidate) => candidate.value === operator)?.needsValue ?? true;

interface TaskMatch {
  root: TaskRootSummary;
  path: number[];
  id: number;
  name: string;
  childCount: number;
}

const CATEGORY_LABELS = {
  general: "General",
  availability: "Availability",
  prerequisites: "Prerequisites",
  objectives: "Objectives",
  failure: "Failure",
  rewards: "Rewards",
  dialogs: "Text and dialogs",
  hierarchy: "Hierarchy",
} as const;

type TaskCategory = keyof typeof CATEGORY_LABELS;

function categoryOf(name: string): TaskCategory {
  const value = name.toLocaleLowerCase();
  if (value.includes("subtask")) return "hierarchy";
  if (value === "texts" || value === "dialogs") return "dialogs";
  if (value.includes("award") || value === "given_items") return "rewards";
  if (value.includes("fail")) return "failure";
  if (value.includes("monster_wanted") || value.includes("item_wanted") || value.includes("interaction_object") || value.startsWith("finish_compare") || value === "summoned_monsters") return "objectives";
  if (value.startsWith("premise") || value.startsWith("team") || value.includes("teammate") || value.includes("member_distance") || value.startsWith("captain_") || value === "all_success" || value === "success_distance") return "prerequisites";
  if (value.includes("timetable") || value === "time_limit" || value === "absolute_time" || value.startsWith("show_by_") || value.startsWith("receive_") || value.startsWith("shared_")) return "availability";
  return "general";
}

function categorize(fields: TaskFieldView[]) {
  const result = new Map<TaskCategory, TaskFieldView[]>();
  const visible = fields.flatMap((field) => field.name === "fixed" && field.children?.length ? field.children : [field]);
  for (const field of visible) {
    const category = categoryOf(field.name);
    const entries = result.get(category) ?? [];
    entries.push(field);
    result.set(category, entries);
  }
  return (Object.keys(CATEGORY_LABELS) as TaskCategory[]).flatMap((key) => {
    const entries = result.get(key);
    return entries?.length ? [{ key, label: CATEGORY_LABELS[key], fields: entries }] : [];
  });
}

function NestedTaskRow({ root, node, selected, expanded, onSelect, onToggle, depth = 1 }: { root: TaskRootSummary; node: TaskTreeNode; selected: string | null; expanded: Set<string>; onSelect: (path: number[]) => void; onToggle: (path: number[]) => void; depth?: number }) {
  const key = branchKey(root, node.path);
  const open = expanded.has(key);
  return <>
    <div className={"task-list-row child" + (key === selected ? " selected" : "")} style={{ paddingLeft: 8 + depth * 16 }}>
      {node.children.length ? <button className="task-disclosure" onClick={() => onToggle(node.path)} title={open ? "Collapse subtasks" : "Expand subtasks"} aria-label={open ? `Collapse ${node.name}` : `Expand ${node.name}`}>{open ? <Minus size={11} /> : <Plus size={11} />}</button> : <span className="task-disclosure-spacer" />}
      <button className="task-list-select" onClick={() => onSelect(node.path)} title={`Task ID ${node.id}`}>
        <span className="mono task-tree-id">{node.id}</span>
        <span className="truncate">{node.name || "(unnamed task)"}</span>
        {!!node.children.length && <span className="task-child-count">{node.children.length}</span>}
      </button>
    </div>
    {open && node.children.map((child) => <NestedTaskRow key={pathKey(child.path)} root={root} node={child} selected={selected} expanded={expanded} onSelect={onSelect} onToggle={onToggle} depth={depth + 1} />)}
  </>;
}

function TaskReferenceView({ reference, onOpen }: { reference: TaskFieldReference; onOpen: (reference: TaskFieldReference) => void }) {
  const linked = reference.kind === "task" && reference.pack !== undefined || reference.kind === "element" && reference.list !== undefined;
  if (linked) {
    return <button className="task-field-reference" onClick={() => onOpen(reference)} title={`Open ${reference.kind} ${reference.id}`}><Link2 size={11} /> {reference.label}</button>;
  }
  if (reference.description && (reference.kind === "skill" || reference.kind === "buff" || reference.kind === "title")) {
    return <span className="task-field-reference static"><ResourceHint kind={reference.kind} name={reference.label} description={reference.description} /></span>;
  }
  return <span className="task-field-reference static" title={reference.description}>{reference.label}</span>;
}

function FieldRow({ field, depth = 0, onReference, onEdit }: { field: TaskFieldView; depth?: number; onReference: (reference: TaskFieldReference) => void; onEdit: (field: TaskFieldView, value: string) => Promise<void> }) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(field.value ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!editing) setValue(field.value ?? "");
  }, [editing, field.value]);
  const commit = async () => {
    setSaving(true);
    setError(null);
    try {
      await onEdit(field, value);
      setEditing(false);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setSaving(false);
    }
  };
  const hasChildren = !!field.children?.length;
  if (hasChildren) {
    return <details className={"task-field-group" + (field.raw ? " raw" : "")} open={depth === 0 && field.name === "fixed"}>
      <summary className="task-field-row" style={{ paddingLeft: 12 + depth * 16 }}>
        <span className="task-field-name">{label(field.name)}</span>
        <span className="task-field-value muted">{field.value}</span>
        <span className="task-field-type mono">{field.ty}</span>
        <span className="task-field-offset mono">0x{field.offset.toString(16).toUpperCase().padStart(4, "0")}</span>
      </summary>
      {field.children!.map((child, index) => <FieldRow key={`${child.name}:${child.offset}:${index}`} field={child} depth={depth + 1} onReference={onReference} onEdit={onEdit} />)}
    </details>;
  }
  const multiline = field.ty.includes("wstring") && ((field.value?.includes("\n") ?? false) || (field.value?.length ?? 0) > 80);
  return <div className={"task-field-row leaf" + (field.raw ? " raw" : "") + (field.changed ? " changed" : "") + (field.editable ? " editable" : "")} style={{ paddingLeft: 28 + depth * 16 }} title={field.editable ? "Click the value to edit" : field.interpretation}>
    <span className="task-field-name">{field.changed && <span className="changed-dot" />} {label(field.name)}</span>
    <span className="task-field-value-wrap">
      {editing ? <span className="task-field-editor">
        {field.ty === "bool8" ? <select value={value} onChange={(event) => setValue(event.target.value)} autoFocus><option value="true">true</option><option value="false">false</option></select>
          : multiline ? <textarea value={value} onChange={(event) => setValue(event.target.value)} autoFocus rows={4} />
          : <input value={value} onChange={(event) => setValue(event.target.value)} autoFocus onKeyDown={(event) => { if (event.key === "Enter") void commit(); if (event.key === "Escape") setEditing(false); }} spellCheck={field.ty.includes("wstring")} />}
        <button className="icon-btn small" onClick={() => void commit()} disabled={saving} title="Apply edit"><Check size={13} /></button>
        <button className="icon-btn small" onClick={() => { setEditing(false); setError(null); }} disabled={saving} title="Cancel"><X size={13} /></button>
        {error && <span className="task-field-edit-error">{error}</span>}
      </span> : <>
        <button className={"task-field-value edit-value" + (field.ty.includes("wstring") ? " text" : " mono")} disabled={!field.editable} onClick={() => field.editable && setEditing(true)}>{field.value ?? ""}{field.editable && <Pencil size={11} />}</button>
        {field.reference && <TaskReferenceView reference={field.reference} onOpen={onReference} />}
      </>}
    </span>
    <span className="task-field-type mono">{field.ty}</span>
    <span className="task-field-offset mono">0x{field.offset.toString(16).toUpperCase().padStart(4, "0")}</span>
    {field.interpretation && <span className="task-field-interpretation mono">{field.interpretation}</span>}
  </div>;
}

export const TasksEditor = forwardRef<TasksEditorHandle, Props>(function TasksEditor({ active, defaultPath, initialState, onStateChange, onOpenElement }, ref) {
  const initialFile = initialState?.loaded ? initialState.summary : null;
  const initialUnsupported = initialState?.loaded ? initialState.unsupported : null;
  const initialRoot = initialFile && initialState?.selection
    ? initialFile.roots.find((root) => root.pack === initialState.selection!.pack && root.root === initialState.selection!.root) ?? null
    : null;
  const [file, setFile] = useState<TasksFileSummary | null>(initialFile);
  const [unsupported, setUnsupported] = useState<TaskSourceInfo | null>(initialUnsupported);
  const [analysis, setAnalysis] = useState<TaskAnalysisReport | null>(initialState?.analysis ?? null);
  const [comparison, setComparison] = useState<TaskIdComparisonReport | null>(initialState?.comparison ?? null);
  const [fieldCandidates, setFieldCandidates] = useState<TaskFieldCandidateReport | null>(initialState?.fieldCandidates ?? null);
  const [layoutPatch, setLayoutPatch] = useState<TaskLayoutPatch | null>(initialState?.layoutPatch ?? null);
  const [referencePath, setReferencePath] = useState<string | null>(initialState?.referencePath ?? null);
  const [baseline, setBaseline] = useState(initialState?.layoutPatch?.baseVersion ?? initialUnsupported?.closestVersion ?? 184);
  const [analysisBusy, setAnalysisBusy] = useState(false);
  const [comparisonBusy, setComparisonBusy] = useState(false);
  const [candidatesBusy, setCandidatesBusy] = useState(false);
  const [layoutBusy, setLayoutBusy] = useState(false);
  const [layoutNote, setLayoutNote] = useState<string | null>(null);
  const [schemaOpen, setSchemaOpen] = useState(false);
  const [arrayOpen, setArrayOpen] = useState(false);
  const [baselineFieldOpen, setBaselineFieldOpen] = useState(false);
  const [candidateDraft, setCandidateDraft] = useState<{ candidate: TaskFieldCandidate; name: string; fieldType: string } | null>(null);
  const [conditionDraft, setConditionDraft] = useState<{ index: number; field: string; rows: TaskLayoutCondition[] } | null>(null);
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query.trim().toLocaleLowerCase());
  const [page, setPage] = useState(0);
  const [pageInput, setPageInput] = useState("1");
  const [selectedRoot, setSelectedRoot] = useState<TaskRootSummary | null>(initialRoot);
  const [selectedPath, setSelectedPath] = useState<number[]>(initialState?.selection?.path ?? []);
  const [detail, setDetail] = useState<TaskDetail | null>(null);
  const [trees, setTrees] = useState<Map<string, TaskTreeNode>>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [taskBusy, setTaskBusy] = useState(false);
  const [searchReport, setSearchReport] = useState<TaskSearchReport | null>(null);
  const [taskIndexReady, setTaskIndexReady] = useState(false);
  const [deletePreview, setDeletePreview] = useState<TaskDeletePreview | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [moveSource, setMoveSource] = useState<TaskSearchEntry | null>(null);
  const [moveBusy, setMoveBusy] = useState(false);
  const [moveError, setMoveError] = useState<string | null>(null);
  const [editState, setEditState] = useState<TaskEditState>(initialState?.edits ?? { changedRoots: [] });
  const [history, setHistory] = useState<TaskHistoryEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [savedNote, setSavedNote] = useState<string | null>(null);
  const autoOpened = useRef<string | null>(initialFile?.path ?? initialUnsupported?.path ?? null);
  const restoring = useRef(!!initialFile);
  const request = useRef(0);

  const selectTask = useCallback(async (root: TaskRootSummary, taskPath: number[], expandAfter = false) => {
    const current = ++request.current;
    setSelectedRoot(root);
    setSelectedPath(taskPath);
    setTaskBusy(true);
    setError(null);
    try {
      const next = await getTask(root.pack, root.root, taskPath);
      if (request.current === current) {
        setDetail(next);
        setTrees((currentTrees) => new Map(currentTrees).set(rootKey(root), next.tree));
        if (expandAfter && next.tree.children.length) {
          setExpanded((currentExpanded) => new Set(currentExpanded).add(branchKey(root, [])));
        }
      }
    } catch (problem) {
      if (request.current === current) setError(String(problem).replace(/^Error: /, ""));
    } finally {
      if (request.current === current) setTaskBusy(false);
    }
  }, []);

  const refreshSelected = useCallback(async () => {
    if (!selectedRoot) return;
    const next = await getTask(selectedRoot.pack, selectedRoot.root, selectedPath);
    setDetail(next);
    setTrees((currentTrees) => new Map(currentTrees).set(rootKey(selectedRoot), next.tree));
    setFile((currentFile) => currentFile ? {
      ...currentFile,
      roots: currentFile.roots.map((root) => root.pack === selectedRoot.pack && root.root === selectedRoot.root ? {
        ...root,
        id: next.tree.id,
        name: next.tree.name,
        childCount: next.tree.children.length,
        byteSize: next.rootBytes,
      } : root),
    } : currentFile);
  }, [selectedPath, selectedRoot]);

  const refreshHistory = useCallback(async () => {
    if (history !== null) setHistory(await getTaskEditHistory());
  }, [history]);

  const runEditAction = useCallback(async (action: () => Promise<TaskEditState>) => {
    setTaskBusy(true);
    setError(null);
    try {
      const state = await action();
      setEditState(state);
      await refreshSelected();
      await refreshHistory();
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
      throw problem;
    } finally {
      setTaskBusy(false);
    }
  }, [refreshHistory, refreshSelected]);

  const editField = useCallback(async (field: TaskFieldView, value: string) => {
    if (!selectedRoot) return;
    await runEditAction(() => editTaskField({ pack: selectedRoot.pack, root: selectedRoot.root, taskPath: selectedPath, fieldPath: field.path, value }));
  }, [runEditAction, selectedPath, selectedRoot]);

  const cloneSelectedSubtree = useCallback(async () => {
    if (!selectedRoot || !selectedPath.length || taskBusy) return;
    setTaskBusy(true);
    setError(null);
    try {
      const report = await cloneTaskSubtree(selectedRoot.pack, selectedRoot.root, selectedPath);
      setEditState(report.state);
      setSelectedPath(report.path);
      const next = await getTask(report.pack, report.root, report.path);
      setDetail(next);
      setTrees((currentTrees) => new Map(currentTrees).set(rootKey(selectedRoot), next.tree));
      setFile((currentFile) => currentFile ? {
        ...currentFile,
        roots: currentFile.roots.map((root) => root.pack === report.pack && root.root === report.root ? {
          ...root,
          id: next.tree.id,
          name: next.tree.name,
          childCount: next.tree.children.length,
          byteSize: next.rootBytes,
        } : root),
      } : currentFile);
      setExpanded((current) => {
        const expanded = new Set(current);
        for (let depth = 0; depth < report.path.length; depth++) expanded.add(branchKey(selectedRoot, report.path.slice(0, depth)));
        return expanded;
      });
      await refreshHistory();
      setSavedNote(report.tasks === 1 ? `Cloned subquest as ID ${report.id}.` : `Cloned ${report.tasks} quests with fresh IDs; new root ID ${report.id}.`);
      window.setTimeout(() => setSavedNote(null), 5000);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setTaskBusy(false);
    }
  }, [refreshHistory, selectedPath, selectedRoot, taskBusy]);

  const cloneSelectedRoot = useCallback(async () => {
    if (!selectedRoot || selectedPath.length || taskBusy || !taskIndexReady) return;
    setTaskBusy(true);
    setError(null);
    try {
      const report = await cloneTaskRoot(selectedRoot.pack, selectedRoot.root);
      const next = await getTask(report.pack, report.root, []);
      const roots = file?.roots ?? [];
      const at = roots.reduce((last, root, index) => root.pack === report.pack ? index + 1 : last, 0);
      const clonedRoot: TaskRootSummary = { ...selectedRoot, index: at, pack: report.pack, root: report.root, id: next.tree.id, name: next.tree.name, childCount: next.tree.children.length, byteSize: next.rootBytes };
      setFile((currentFile) => currentFile ? {
        ...currentFile,
        rootCount: currentFile.rootCount + 1,
        roots: [...currentFile.roots.slice(0, at), clonedRoot, ...currentFile.roots.slice(at)].map((root, index) => ({ ...root, index })),
      } : currentFile);
      setSelectedRoot(clonedRoot);
      setSelectedPath([]);
      setDetail(next);
      setTrees((currentTrees) => new Map(currentTrees).set(rootKey(clonedRoot), next.tree));
      await refreshHistory();
      setSavedNote(report.tasks === 1 ? `Cloned task as ID ${report.id}.` : `Cloned task tree with ${report.tasks} fresh IDs; new root ID ${report.id}.`);
      window.setTimeout(() => setSavedNote(null), 5000);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setTaskBusy(false);
    }
  }, [file?.roots, refreshHistory, selectedPath.length, selectedRoot, taskBusy, taskIndexReady]);

  const openMoveSelected = useCallback(() => {
    if (!selectedRoot || !detail || !selectedPath.length || taskBusy) return;
    setMoveSource({
      pack: selectedRoot.pack,
      root: selectedRoot.root,
      path: selectedPath,
      id: detail.id,
      name: detail.name,
      childCount: detail.tree.children.length,
    });
    setMoveError(null);
  }, [detail, selectedPath, selectedRoot, taskBusy]);

  const confirmMoveSelected = useCallback(async (destination: TaskSearchEntry) => {
    if (!moveSource || moveBusy) return;
    setMoveBusy(true);
    setTaskBusy(true);
    setMoveError(null);
    try {
      const report = await moveTaskSubtree(moveSource, destination);
      setEditState(report.state);
      const affected = [
        { pack: moveSource.pack, root: moveSource.root },
        { pack: destination.pack, root: destination.root },
      ].filter((value, index, values) => values.findIndex((other) => other.pack === value.pack && other.root === value.root) === index);
      const roots = await Promise.all(affected.map(async (value) => ({ ...value, detail: await getTask(value.pack, value.root, []) })));
      const byRoot = new Map(roots.map((value) => [`${value.pack}:${value.root}`, value.detail]));
      setTrees((currentTrees) => {
        const next = new Map(currentTrees);
        for (const value of roots) next.set(`${value.pack}:${value.root}`, value.detail.tree);
        return next;
      });
      setFile((currentFile) => currentFile ? {
        ...currentFile,
        roots: currentFile.roots.map((root) => {
          const refreshed = byRoot.get(`${root.pack}:${root.root}`);
          return refreshed ? { ...root, id: refreshed.tree.id, name: refreshed.tree.name, childCount: refreshed.tree.children.length, byteSize: refreshed.rootBytes } : root;
        }),
      } : currentFile);
      const sourceRoot = file?.roots.find((root) => root.pack === report.pack && root.root === report.root);
      if (!sourceRoot) throw new Error("The destination task root is no longer available.");
      const refreshedRoot = byRoot.get(`${report.pack}:${report.root}`);
      const selectedRoot = refreshedRoot ? { ...sourceRoot, id: refreshedRoot.tree.id, name: refreshedRoot.tree.name, childCount: refreshedRoot.tree.children.length, byteSize: refreshedRoot.rootBytes } : sourceRoot;
      const selected = await getTask(report.pack, report.root, report.path);
      setSelectedRoot(selectedRoot);
      setSelectedPath(report.path);
      setDetail(selected);
      setTrees((currentTrees) => new Map(currentTrees).set(rootKey(selectedRoot), selected.tree));
      setMoveSource(null);
      await refreshHistory();
      setSavedNote(report.tasks === 1 ? `Moved subquest ${report.id}. Undo is available.` : `Moved ${report.tasks} quests as one subtree. Undo is available.`);
      window.setTimeout(() => setSavedNote(null), 5000);
    } catch (problem) {
      setMoveError(String(problem).replace(/^Error: /, ""));
    } finally {
      setTaskBusy(false);
      setMoveBusy(false);
    }
  }, [file?.roots, moveBusy, moveSource, refreshHistory]);

  const inspectDeleteSelected = useCallback(async () => {
    if (!selectedRoot || !selectedPath.length || taskBusy) return;
    setTaskBusy(true);
    setError(null);
    try {
      setDeletePreview(await previewDeleteTaskSubtree(selectedRoot.pack, selectedRoot.root, selectedPath));
      setDeleteError(null);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setTaskBusy(false);
    }
  }, [selectedPath, selectedRoot, taskBusy]);

  const confirmDeleteSelected = useCallback(async () => {
    if (!deletePreview || deleteBusy) return;
    setDeleteBusy(true);
    setDeleteError(null);
    try {
      const report = await deleteTaskSubtree(deletePreview);
      setEditState(report.state);
      const root = file?.roots.find((candidate) => candidate.pack === report.pack && candidate.root === report.root) ?? selectedRoot;
      if (!root) throw new Error("The changed task root is no longer available.");
      setSelectedRoot(root);
      setSelectedPath(report.path);
      const next = await getTask(report.pack, report.root, report.path);
      setDetail(next);
      setTrees((currentTrees) => new Map(currentTrees).set(rootKey(root), next.tree));
      setFile((currentFile) => currentFile ? {
        ...currentFile,
        roots: currentFile.roots.map((candidate) => candidate.pack === report.pack && candidate.root === report.root ? {
          ...candidate,
          id: next.tree.id,
          name: next.tree.name,
          childCount: next.tree.children.length,
          byteSize: next.rootBytes,
        } : candidate),
      } : currentFile);
      setDeletePreview(null);
      await refreshHistory();
      setSavedNote(report.tasks === 1 ? `Deleted subquest ${report.id}. Undo is available.` : `Deleted ${report.tasks} quests from subtree ${report.id}. Undo is available.`);
      window.setTimeout(() => setSavedNote(null), 5000);
    } catch (problem) {
      setDeleteError(String(problem).replace(/^Error: /, ""));
    } finally {
      setDeleteBusy(false);
    }
  }, [deleteBusy, deletePreview, file?.roots, refreshHistory, selectedRoot]);

  const undo = useCallback(() => void runEditAction(undoTaskEdit).catch(() => {}), [runEditAction]);
  const redo = useCallback(() => void runEditAction(redoTaskEdit).catch(() => {}), [runEditAction]);
  const revertAll = useCallback(() => {
    if (!editState.changedRoots.length || !window.confirm(`Put ${editState.changedRoots.length} changed task root(s) back as they were opened? Undo can bring the edits back.`)) return;
    void runEditAction(revertTaskEdits).catch(() => {});
  }, [editState.changedRoots.length, runEditAction]);

  const load = useCallback(async (path: string, skipUnsavedPrompt = false) => {
    if (!skipUnsavedPrompt && file && editState.changedRoots.length && !window.confirm("Open another tasks.data file and discard the current unsaved changes?")) return;
    const current = ++request.current;
    setBusy(true);
    setTaskBusy(false);
    setError(null);
    setFile(null);
    setUnsupported(null);
    setAnalysis(null);
    setComparison(null);
    setFieldCandidates(null);
    setLayoutPatch(null);
    setCandidateDraft(null);
    setConditionDraft(null);
    setLayoutNote(null);
    setSchemaOpen(false);
    setArrayOpen(false);
    setBaselineFieldOpen(false);
    setReferencePath(null);
    setDetail(null);
    setSelectedRoot(null);
    setSelectedPath([]);
    setTrees(new Map());
    setExpanded(new Set());
    setSearchReport(null);
    setTaskIndexReady(false);
    setDeletePreview(null);
    setDeleteBusy(false);
    setDeleteError(null);
    setMoveSource(null);
    setMoveBusy(false);
    setMoveError(null);
    setEditState({ changedRoots: [] });
    setHistory(null);
    try {
      const sourceVersion = await taskSourceVersion(path);
      if (!sourceVersion.supported) {
        const source = await inspectTasks(path);
        if (request.current !== current) return;
        setUnsupported(source);
        try {
          const patch = await getTaskLayoutPatch(source.version);
          if (request.current !== current) return;
          setLayoutPatch(patch);
          setBaseline(patch?.baseVersion ?? source.closestVersion);
        } catch (problem) {
          if (request.current === current) setError(String(problem).replace(/^Error: /, ""));
        }
        setQuery("");
        setPage(0);
        return;
      }
      const opened = await openTasks(path);
      if (request.current !== current) return;
      setFile(opened);
      setUnsupported(null);
      setQuery("");
      setPage(0);
      setBusy(false);
      if (opened.roots[0]) void selectTask(opened.roots[0], []);
    } catch (problem) {
      try {
        const source = await inspectTasks(path);
        if (request.current === current) {
          setUnsupported(source);
          setError(null);
          try {
            const patch = await getTaskLayoutPatch(source.version);
            if (request.current !== current) return;
            setLayoutPatch(patch);
            setBaseline(patch?.baseVersion ?? source.closestVersion);
          } catch (patchProblem) {
            if (request.current === current) setError(String(patchProblem).replace(/^Error: /, ""));
          }
        }
      } catch {
        if (request.current === current) setError(String(problem).replace(/^Error: /, ""));
      }
    } finally {
      if (request.current === current) setBusy(false);
    }
  }, [editState.changedRoots.length, file, selectTask]);

  const choose = useCallback(async () => {
    const picked = await open({
      multiple: false,
      directory: false,
      defaultPath: file?.path || unsupported?.path || defaultPath || undefined,
      title: "Open tasks.data",
      filters: [{ name: "tasks.data", extensions: ["data"] }],
    });
    if (typeof picked === "string") void load(picked);
  }, [defaultPath, file?.path, load, unsupported?.path]);

  const openSave = useCallback(() => file && !busy && !taskBusy && setSaving(true), [busy, file, taskBusy]);
  useImperativeHandle(ref, () => ({ choose, openPath: (path) => void load(path), undo, redo, revertAll, save: openSave }), [choose, load, openSave, redo, revertAll, undo]);

  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey)) return;
      const key = event.key.toLowerCase();
      if (key !== "o" && key !== "s" && key !== "z" && key !== "y") return;
      const typing = event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement || event.target instanceof HTMLSelectElement;
      if (typing && (key === "z" || key === "y")) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      if (key === "o") void choose();
      if (key === "s") openSave();
      if (key === "z") undo();
      if (key === "y") redo();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [active, choose, openSave, redo, undo]);

  useEffect(() => {
    if (!active || !defaultPath || file || unsupported || autoOpened.current === defaultPath) return;
    autoOpened.current = defaultPath;
    void load(defaultPath);
  }, [active, defaultPath, file, load, unsupported]);

  useEffect(() => {
    if (!active || !file || !restoring.current) return;
    restoring.current = false;
    const root = selectedRoot ?? file.roots[0];
    if (root) void selectTask(root, selectedRoot ? selectedPath : []);
  }, [active, file, selectTask, selectedPath, selectedRoot]);

  useEffect(() => {
    onStateChange({
      loaded: !!file || !!unsupported,
      path: file?.path ?? unsupported?.path ?? null,
      summary: file,
      unsupported,
      analysis,
      comparison,
      fieldCandidates,
      layoutPatch,
      referencePath,
      dirty: editState.changedRoots.length > 0,
      canUndo: !!editState.undo,
      canRedo: !!editState.redo,
      edits: editState,
      selection: selectedRoot ? { pack: selectedRoot.pack, root: selectedRoot.root, path: selectedPath } : null,
    });
  }, [analysis, comparison, editState, fieldCandidates, file, layoutPatch, onStateChange, referencePath, selectedPath, selectedRoot, unsupported]);

  const runAnalysis = useCallback(async () => {
    if (!unsupported || analysisBusy) return;
    setAnalysisBusy(true);
    setError(null);
    try {
      if (layoutPatch) {
        const report = await analyzeTaskLayoutPatch(unsupported.path);
        setLayoutPatch(report.patch);
        setAnalysis(report.analysis);
      } else {
        setAnalysis(await analyzeTasks(unsupported.path, baseline));
      }
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setAnalysisBusy(false);
    }
  }, [analysisBusy, baseline, layoutPatch, unsupported]);

  const acceptLayout = useCallback(async () => {
    if (!unsupported || !analysis?.exactRoundTrip || layoutBusy) return;
    setLayoutBusy(true);
    setError(null);
    setLayoutNote(null);
    try {
      await verifyTaskLayout(unsupported.path, layoutPatch?.baseVersion ?? baseline);
      await load(unsupported.path, true);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setLayoutBusy(false);
    }
  }, [analysis?.exactRoundTrip, baseline, layoutBusy, layoutPatch?.baseVersion, load, unsupported]);

  const reopenLayoutAnalyzer = useCallback(async () => {
    if (!file?.userLayout || layoutBusy) return;
    if (editState.changedRoots.length && !window.confirm("Discard the current unsaved task edits and return this user layout to analysis mode?")) return;
    setLayoutBusy(true);
    setError(null);
    try {
      await editTaskLayout(file.version);
      await load(file.path, true);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setLayoutBusy(false);
    }
  }, [editState.changedRoots.length, file, layoutBusy, load]);

  const chooseComparison = useCallback(async () => {
    if (!unsupported || comparisonBusy) return;
    const picked = await open({
      multiple: false,
      directory: false,
      defaultPath: referencePath ?? unsupported.path,
      title: "Choose an older supported tasks.data",
      filters: [{ name: "tasks.data", extensions: ["data"] }],
    });
    if (typeof picked !== "string") return;
    setReferencePath(picked);
    setComparison(null);
    setFieldCandidates(null);
    setComparisonBusy(true);
    setError(null);
    try {
      setComparison(await compareTaskIds(unsupported.path, picked));
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setComparisonBusy(false);
    }
  }, [comparisonBusy, referencePath, unsupported]);

  const findFieldCandidates = useCallback(async () => {
    if (!unsupported || !referencePath || candidatesBusy) return;
    setCandidatesBusy(true);
    setError(null);
    try {
      setFieldCandidates(await scoreTaskFields(unsupported.path, referencePath));
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setCandidatesBusy(false);
    }
  }, [candidatesBusy, referencePath, unsupported]);

  const addCandidate = useCallback(async () => {
    if (!unsupported || !candidateDraft || layoutBusy) return;
    setLayoutBusy(true);
    setError(null);
    setLayoutNote(null);
    try {
      const report = await addTaskLayoutField(unsupported.path, layoutPatch?.baseVersion ?? baseline, candidateDraft.candidate.structure, candidateDraft.candidate.afterField, candidateDraft.name.trim(), candidateDraft.candidate.width, candidateDraft.fieldType);
      setLayoutPatch(report.patch);
      setAnalysis(report.analysis);
      setFieldCandidates(null);
      setCandidateDraft(null);
      setConditionDraft(null);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setLayoutBusy(false);
    }
  }, [baseline, candidateDraft, layoutBusy, layoutPatch?.baseVersion, unsupported]);

  const addCountedArray = useCallback(async (draft: TaskCountedArrayDraft) => {
    if (!unsupported || layoutBusy) return false;
    setLayoutBusy(true);
    setError(null);
    setLayoutNote(null);
    try {
      const report = await addTaskLayoutCountedArray(unsupported.path, layoutPatch?.baseVersion ?? baseline, draft.structure, draft.afterField, draft.name, draft.countField, draft.itemType);
      setLayoutPatch(report.patch);
      setAnalysis(report.analysis);
      setFieldCandidates(null);
      setCandidateDraft(null);
      setConditionDraft(null);
      return true;
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
      return false;
    } finally {
      setLayoutBusy(false);
    }
  }, [baseline, layoutBusy, layoutPatch?.baseVersion, unsupported]);

  const changeBaselineField = useCallback(async (draft: TaskBaselineFieldDraft) => {
    if (!unsupported || layoutBusy) return false;
    setLayoutBusy(true);
    setError(null);
    setLayoutNote(null);
    try {
      const baseVersion = layoutPatch?.baseVersion ?? baseline;
      const report = draft.mode === "remove"
        ? await removeTaskLayoutField(unsupported.path, baseVersion, draft.structure, draft.field)
        : await replaceTaskLayoutFieldType(unsupported.path, baseVersion, draft.structure, draft.field, draft.fieldType);
      setLayoutPatch(report.patch);
      setAnalysis(report.analysis);
      setFieldCandidates(null);
      setCandidateDraft(null);
      setConditionDraft(null);
      return true;
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
      return false;
    } finally {
      setLayoutBusy(false);
    }
  }, [baseline, layoutBusy, layoutPatch?.baseVersion, unsupported]);

  const removePatchOperation = useCallback(async (index: number) => {
    if (!unsupported || layoutBusy) return;
    setLayoutBusy(true);
    setError(null);
    setLayoutNote(null);
    try {
      const report = await removeTaskLayoutOperation(unsupported.path, index);
      setLayoutPatch(report.patch.operations.length ? report.patch : null);
      setAnalysis(report.analysis);
      setFieldCandidates(null);
      setCandidateDraft(null);
      setConditionDraft(null);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setLayoutBusy(false);
    }
  }, [layoutBusy, unsupported]);

  const changePatchOperationType = useCallback(async (index: number, fieldType: string) => {
    if (!unsupported || layoutBusy) return;
    setLayoutBusy(true);
    setError(null);
    setLayoutNote(null);
    try {
      const report = await setTaskLayoutOperationType(unsupported.path, index, fieldType);
      setLayoutPatch(report.patch);
      setAnalysis(report.analysis);
      setFieldCandidates(null);
      setCandidateDraft(null);
      setConditionDraft(null);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setLayoutBusy(false);
    }
  }, [layoutBusy, unsupported]);

  const applyPatchConditions = useCallback(async () => {
    if (!unsupported || !conditionDraft || layoutBusy) return;
    setLayoutBusy(true);
    setError(null);
    setLayoutNote(null);
    try {
      const report = await setTaskLayoutOperationConditions(unsupported.path, conditionDraft.index, conditionDraft.rows);
      setLayoutPatch(report.patch);
      setAnalysis(report.analysis);
      setFieldCandidates(null);
      setCandidateDraft(null);
      setConditionDraft(null);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setLayoutBusy(false);
    }
  }, [conditionDraft, layoutBusy, unsupported]);

  const exportLayoutPatch = useCallback(async () => {
    if (!unsupported || !layoutPatch || layoutBusy) return;
    const target = await save({
      defaultPath: unsupported.path.replace(/tasks\.data$/i, `tasks-v${unsupported.version}-layout.json`),
      title: `Export tasks.data v${unsupported.version} layout patch`,
      filters: [{ name: "JD IDE task layout", extensions: ["json"] }],
    });
    if (typeof target !== "string") return;
    setLayoutBusy(true);
    setError(null);
    setLayoutNote(null);
    try {
      const report = await exportTaskLayoutPatch(unsupported.version, target);
      setLayoutNote(`Exported ${report.operations} operation${report.operations === 1 ? "" : "s"} to ${report.path.split(/[\\/]/).pop()}.`);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setLayoutBusy(false);
    }
  }, [layoutBusy, layoutPatch, unsupported]);

  const importLayoutPatch = useCallback(async () => {
    if (!unsupported || layoutBusy) return;
    const source = await open({
      multiple: false,
      directory: false,
      defaultPath: layoutPatch?.path ?? unsupported.path,
      title: `Import tasks.data v${unsupported.version} layout patch`,
      filters: [{ name: "JD IDE task layout", extensions: ["json"] }],
    });
    if (typeof source !== "string") return;
    if (layoutPatch && !window.confirm(`Replace the current v${unsupported.version} user layout with ${source.split(/[\\/]/).pop()}? The imported patch will be validated against every task root first.`)) return;
    setLayoutBusy(true);
    setError(null);
    setLayoutNote(null);
    try {
      const report = await importTaskLayoutPatch(unsupported.path, source);
      setLayoutPatch(report.patch);
      setAnalysis(report.analysis);
      setBaseline(report.patch.baseVersion);
      setFieldCandidates(null);
      setCandidateDraft(null);
      setConditionDraft(null);
      if (comparison && comparison.reference.version !== report.patch.baseVersion) {
        setComparison(null);
        setReferencePath(null);
      }
      setLayoutNote(`Imported ${report.patch.operations.length} operation${report.patch.operations.length === 1 ? "" : "s"} and checked every task root.`);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setLayoutBusy(false);
    }
  }, [comparison, layoutBusy, layoutPatch, unsupported]);

  useEffect(() => {
    if (!file || !deferredQuery) {
      setSearchReport(null);
      return;
    }
    setSearchReport(null);
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const run = async () => {
      try {
        const report = await searchTasks(deferredQuery);
        if (cancelled) return;
        setSearchReport(report);
        if (!report.indexed && !report.error) timer = setTimeout(run, 500);
      } catch (problem) {
        if (!cancelled) setSearchReport({ indexed: false, error: String(problem).replace(/^Error: /, ""), total: 0, matches: [] });
      }
    };
    void run();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [deferredQuery, file]);

  useEffect(() => {
    if (!file) {
      setTaskIndexReady(false);
      return;
    }
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const check = async () => {
      try {
        const report = await searchTasks("", 0);
        if (cancelled) return;
        setTaskIndexReady(report.indexed);
        if (!report.indexed && !report.error) timer = setTimeout(check, 500);
      } catch {
        if (!cancelled) timer = setTimeout(check, 1000);
      }
    };
    void check();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [file]);

  const matches = useMemo<TaskMatch[]>(() => {
    if (!file) return [];
    if (!deferredQuery) return file.roots.map((root) => ({ root, path: [], id: root.id, name: root.name, childCount: root.childCount }));
    const rootByKey = new Map(file.roots.map((root) => [rootKey(root), root]));
    return (searchReport?.matches ?? []).flatMap((task) => {
      const root = rootByKey.get(`${task.pack}:${task.root}`);
      return root ? [{ root, path: task.path, id: task.id, name: task.name, childCount: task.childCount }] : [];
    });
  }, [deferredQuery, file, searchReport]);
  const pages = Math.max(1, Math.ceil(matches.length / PAGE_SIZE));
  const shown = matches.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);

  useEffect(() => setPage(0), [deferredQuery]);
  useEffect(() => {
    const clamped = Math.min(page, pages - 1);
    if (clamped !== page) setPage(clamped);
    setPageInput(String(clamped + 1));
  }, [page, pages]);

  const applyPageInput = () => {
    const parsed = Number(pageInput);
    const next = Number.isFinite(parsed) ? Math.max(1, Math.min(pages, Math.trunc(parsed))) : page + 1;
    setPage(next - 1);
    setPageInput(String(next));
  };

  const toggleBranch = (root: TaskRootSummary, taskPath: number[]) => {
    const key = branchKey(root, taskPath);
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const selectedBranch = selectedRoot ? branchKey(selectedRoot, selectedPath) : null;
  const changedRoots = useMemo(() => new Set(editState.changedRoots.map((root) => `${root.pack}:${root.root}`)), [editState.changedRoots]);
  const categories = useMemo(() => categorize(detail?.fields ?? []), [detail]);
  const followReference = useCallback((reference: TaskFieldReference) => {
    if (reference.kind === "task" && reference.pack !== undefined && reference.root !== undefined && file) {
      const root = file.roots.find((candidate) => candidate.pack === reference.pack && candidate.root === reference.root);
      if (root) void selectTask(root, reference.path ?? []);
    } else if (reference.kind === "element" && reference.list !== undefined && reference.row !== undefined) {
      onOpenElement?.(reference.list, reference.row);
    }
  }, [file, onOpenElement, selectTask]);

  if (!file) {
    if (unsupported) {
      const issue = analysis?.firstIssue;
      const scoringBlocked = !!layoutPatch?.operations.some((operation) => operation.kind !== "insert" || !operation.width);
      return <><section className="tasks-pane task-analyzer-pane" aria-busy={busy || analysisBusy || comparisonBusy || candidatesBusy || layoutBusy}>
        <header className="tasks-head">
          <div><h2>Task layout analyzer <span className="tag warn">unverified layout</span></h2><div className="tasks-file-line"><span className="mono truncate" title={unsupported.path}>{unsupported.path}</span><span className="path-data-badge"><b>Version:</b> v{unsupported.version}</span><span className="path-data-badge"><b>Roots:</b> {count(unsupported.rootCount)}</span><span className="path-data-badge"><b>Packs:</b> {unsupported.packCount}</span><span className="path-data-badge"><b>Size:</b> {bytes(unsupported.size)}</span></div></div>
          <span className="tasks-integrity" title="The index, pack headers, root offsets and stored MD5 values are valid"><ShieldCheck size={14}/> Container verified</span>
          <button className="btn" onClick={() => setSchemaOpen(true)} disabled={layoutBusy}><Braces size={14}/> Task schema</button>
          <button className="btn" onClick={choose} disabled={busy || analysisBusy || comparisonBusy || candidatesBusy || layoutBusy}><FolderOpen size={14}/> Open…</button>
        </header>
        <div className="task-analyzer-scroll">
          <section className="task-analyzer-intro">
            <AlertTriangle size={22}/><div><h3>{unsupported.supported ? `The v${unsupported.version} schema could not decode this task set` : `tasks.data v${unsupported.version} has no verified layout`}</h3><p>Analysis is read-only. It applies an older known schema to every root and reports exactly where that baseline stops matching. Saving stays disabled.</p></div>
          </section>
          <section className="task-analyzer-controls">
            <label>Older schema <select value={baseline} onChange={(event) => { setBaseline(Number(event.target.value)); setAnalysis(null); }} disabled={analysisBusy || !!layoutPatch}>{unsupported.supportedVersions.map((version) => <option value={version} key={version}>v{version}{version === unsupported.closestVersion ? " · closest" : ""}</option>)}</select></label>
            <button className="btn primary" onClick={() => void runAnalysis()} disabled={analysisBusy || layoutBusy}>{analysisBusy ? <Loader2 size={14} className="spin"/> : <FlaskConical size={14}/>} {analysisBusy ? "Analyzing every root…" : layoutPatch ? "Analyze patched layout" : "Analyze layout"}</button>
            <button className="btn" onClick={() => setArrayOpen(true)} disabled={layoutBusy || analysisBusy}><Plus size={14}/> Counted array…</button>
            <button className="btn" onClick={() => setBaselineFieldOpen(true)} disabled={layoutBusy || analysisBusy}><Pencil size={14}/> Baseline field…</button>
            <button className="btn" onClick={() => void importLayoutPatch()} disabled={layoutBusy || analysisBusy}><Upload size={14}/> Import patch…</button>
            <span className="muted small">No data is changed.</span>
          </section>
          {error && <div className="path-data-message error">{error}</div>}
          {layoutNote && <div className="path-data-message ok">{layoutNote}</div>}
          {analysis && <>
            <section className="task-analysis-metrics">
              <div><span>Exact roots</span><b>{count(analysis.exactRoots)} / {count(analysis.source.rootCount)}</b><small>{analysis.rootCoverage.toFixed(2)}%</small></div>
              <div><span>Decoded bytes</span><b>{bytes(analysis.decodedBytes)} / {bytes(analysis.totalBytes)}</b><small>{analysis.byteCoverage.toFixed(2)}%</small></div>
              <div><span>Trailing roots</span><b>{count(analysis.trailingRoots)}</b><small>baseline ends early</small></div>
              <div><span>Failed roots</span><b>{count(analysis.failedRoots)}</b><small>structural failure</small></div>
            </section>
            {analysis.exactRoundTrip ? <div className="task-analysis-result exact"><ShieldCheck size={17}/><div><b>Every root matches the v{analysis.baselineVersion} layout byte-for-byte.</b><span>Accepting reruns the whole-file check, stores this exact schema, and opens the normal task editor.</span></div><button className="btn primary" onClick={() => void acceptLayout()} disabled={layoutBusy}>{layoutBusy ? <Loader2 size={14} className="spin"/> : <Check size={14}/>} Accept layout and open editor</button></div>
              : issue && <div className="task-analysis-result issue"><BarChart3 size={17}/><div><b>First stopping point: pack {issue.pack + 1}, root {issue.root + 1}, offset 0x{issue.offset.toString(16).toUpperCase()}</b><span className="mono">{issue.message}</span><span>{bytes(issue.offset)} of this {bytes(issue.rootBytes)} root was reached before the failure.</span></div></div>}
            <section className="task-pack-coverage"><header><b>Coverage by pack</b><span>Baseline v{analysis.baselineVersion}</span></header><div className="task-pack-coverage-head"><span>Pack</span><span>Exact roots</span><span>Trailing</span><span>Failed</span><span>Decoded bytes</span><span>Coverage</span></div>{analysis.packs.map((pack) => <div className={pack.failedRoots || pack.trailingRoots ? "has-issue" : ""} key={pack.pack}><span className="mono">tasks.data{pack.pack + 1}</span><span>{pack.exactRoots} / {pack.roots}</span><span>{pack.trailingRoots}</span><span>{pack.failedRoots}</span><span>{bytes(pack.decodedBytes)} / {bytes(pack.bytes)}</span><span>{(pack.bytes ? pack.decodedBytes * 100 / pack.bytes : 100).toFixed(2)}%</span></div>)}</section>
          </>}
          {layoutPatch && <section className="task-layout-patch">
            <header><div><b>User task layout patch</b><span>v{layoutPatch.taskVersion} based on v{layoutPatch.baseVersion} · stored at {layoutPatch.path}</span></div><button className="btn small" onClick={() => setBaselineFieldOpen(true)} disabled={layoutBusy}><Pencil size={13}/> Baseline field…</button><button className="btn small" onClick={() => setArrayOpen(true)} disabled={layoutBusy}><Plus size={13}/> Counted array…</button><button className="btn small" onClick={() => void exportLayoutPatch()} disabled={layoutBusy}><Download size={13}/> Export…</button><span className="tag warn">unverified</span></header>
            <div className="task-layout-patch-head"><span>Field</span><span>Operation</span><span>Location</span><span>Type</span><span>Condition</span><span/></div>
            {layoutPatch.operations.map((operation) => <div className="task-layout-patch-row" key={`${operation.index}:${operation.field}`}>
              <span className="mono">{operation.field}</span>
              <span>{operation.kind}</span>
              <span className="mono truncate" title={`${operation.structure}.${operation.afterField ?? ""}`}>{operation.structure}.{operation.afterField ?? "—"}</span>
              {operation.kind === "insert" && operation.width && operation.fieldType
                ? <select value={operation.fieldType} onChange={(event) => void changePatchOperationType(operation.index, event.target.value)} disabled={layoutBusy} title={`${operation.width} bytes`}>{fixedTypeOptions(operation.width).map((type) => <option value={type} key={type}>{type}</option>)}</select>
                : <span>{operation.fieldType ?? "—"}</span>}
              {operation.kind === "insert"
                ? <button className={`btn small task-condition-button${operation.conditions.length ? " active" : ""}`} onClick={() => { setCandidateDraft(null); setConditionDraft({ index: operation.index, field: operation.field, rows: operation.conditions.map((condition) => ({ ...condition })) }); }} disabled={layoutBusy} title={operation.conditions.map((condition) => condition.label).join(" and ") || "Field is always present"}><GitBranch size={12}/>{operation.conditions.length ? operation.conditions.map((condition) => condition.label).join(" · ") : "Always"}</button>
                : <span>—</span>}
              <button className="icon-btn" onClick={() => void removePatchOperation(operation.index)} disabled={layoutBusy} title={`Remove ${operation.field}`}><X size={14}/></button>
            </div>)}
            {conditionDraft && <div className="task-condition-draft">
              <header><div><b>Read <span className="mono">{conditionDraft.field}</span> only when</b><span>Every row must match. Controller fields must be earlier integers in the same structure.</span></div><button className="icon-btn" onClick={() => setConditionDraft(null)} disabled={layoutBusy} aria-label="Close condition editor"><X size={14}/></button></header>
              {conditionDraft.rows.map((condition, index) => <div className="task-condition-row" key={index}>
                <input value={condition.field} onChange={(event) => setConditionDraft({ ...conditionDraft, rows: conditionDraft.rows.map((row, rowIndex) => rowIndex === index ? { ...row, field: event.target.value } : row) })} placeholder="controller_field" aria-label={`Condition ${index + 1} controller field`}/>
                <select value={condition.operator} onChange={(event) => { const operator = event.target.value as TaskLayoutCondition["operator"]; setConditionDraft({ ...conditionDraft, rows: conditionDraft.rows.map((row, rowIndex) => rowIndex === index ? { ...row, operator, value: conditionNeedsValue(operator) ? row.value ?? "" : undefined } : row) }); }} aria-label={`Condition ${index + 1} operator`}>{CONDITION_OPERATORS.map((operator) => <option value={operator.value} key={operator.value}>{operator.label}</option>)}</select>
                {conditionNeedsValue(condition.operator) ? <input value={condition.value ?? ""} onChange={(event) => setConditionDraft({ ...conditionDraft, rows: conditionDraft.rows.map((row, rowIndex) => rowIndex === index ? { ...row, value: event.target.value } : row) })} placeholder={condition.operator === "one_of" ? "1, 2, 3" : condition.operator.startsWith("bits_") ? "0x01" : "value"} aria-label={`Condition ${index + 1} value`}/> : <span className="muted">No value needed</span>}
                <button className="icon-btn" onClick={() => setConditionDraft({ ...conditionDraft, rows: conditionDraft.rows.filter((_, rowIndex) => rowIndex !== index) })} disabled={layoutBusy} title="Remove condition"><X size={13}/></button>
              </div>)}
              {!conditionDraft.rows.length && <div className="empty-note">No conditions. This field is read for every record.</div>}
              <footer><button className="btn small" onClick={() => setConditionDraft({ ...conditionDraft, rows: [...conditionDraft.rows, { field: "", operator: "non_zero", label: "" }] })} disabled={layoutBusy || conditionDraft.rows.length >= 16}><Plus size={12}/> Add condition</button><span className="spacer"/><button className="btn small" onClick={() => setConditionDraft(null)} disabled={layoutBusy}>Cancel</button><button className="btn primary small" onClick={() => void applyPatchConditions()} disabled={layoutBusy}>{layoutBusy ? <Loader2 size={13} className="spin"/> : <Check size={13}/>} Apply and analyze</button></footer>
            </div>}
            <footer>Adding, removing, changing a field type or changing its conditions reruns the entire file. Type changes keep the same byte width. Exact whole-file coverage enables the Accept layout action above.</footer>
          </section>}
          <section className="task-id-comparison">
            <header><div><b>Compare matching root-task IDs</b><span>Choose an older supported task set. Repeated record-size deltas are evidence of fields added to the newer task structure.</span></div><button className="btn" onClick={() => void chooseComparison()} disabled={comparisonBusy}>{comparisonBusy ? <Loader2 size={14} className="spin"/> : <FolderOpen size={14}/>} {comparisonBusy ? "Comparing task sets…" : comparison ? "Choose another…" : "Choose older tasks.data…"}</button></header>
            {referencePath && <div className="task-compare-path mono truncate" title={referencePath}>{referencePath}</div>}
            {comparison && <>
              <div className="task-compare-summary"><div><span>Matching IDs</span><b>{count(comparison.matchedIds)}</b><small>{(comparison.targetRoots ? comparison.matchedIds * 100 / comparison.targetRoots : 100).toFixed(2)}% of newer roots</small></div><div><span>Same size</span><b>{count(comparison.sameSize)}</b><small>complete root bytes</small></div><div><span>Larger / smaller</span><b>{count(comparison.grown)} / {count(comparison.shrunk)}</b><small>newer root records</small></div><div><span>Only in newer / older</span><b>{count(comparison.targetOnly)} / {count(comparison.referenceOnly)}</b><small>{comparison.duplicateIds ? `${count(comparison.duplicateIds)} duplicate IDs excluded` : "unique IDs"}</small></div></div>
              <div className="task-compare-columns">
                <section className="task-size-patterns"><header><b>Root-size patterns</b><span>{comparison.patternCount} pattern{comparison.patternCount === 1 ? "" : "s"}</span></header><div className="task-size-patterns-head"><span>Older</span><span>Newer</span><span>Delta</span><span>IDs</span><span>Examples</span></div>{comparison.sizePatterns.map((pattern) => <div className={pattern.delta ? "changed" : ""} key={`${pattern.referenceBytes}:${pattern.targetBytes}`}><span>{bytes(pattern.referenceBytes)}</span><span>{bytes(pattern.targetBytes)}</span><span className={pattern.delta > 0 ? "positive" : pattern.delta < 0 ? "negative" : ""}>{pattern.delta > 0 ? "+" : ""}{pattern.delta.toLocaleString()} B</span><b>{count(pattern.count)}</b><span className="mono truncate" title={pattern.exampleIds.join(", ")}>{pattern.exampleIds.join(", ")}</span></div>)}{comparison.patternsTruncated && <footer>Showing the {comparison.sizePatterns.length} most common of {comparison.patternCount} patterns.</footer>}</section>
                <section className="task-id-differences"><header><b>ID and name differences</b><span>{count(comparison.differenceCount)}</span></header>{comparison.differences.length ? <><div className="task-id-differences-head"><span>ID</span><span>Kind</span><span>Newer</span><span>Older</span></div>{comparison.differences.map((row, index) => <div key={`${row.kind}:${row.id}:${index}`}><span className="mono">{row.id}</span><span className={`tag ${row.kind}`}>{row.kind.replaceAll("_", " ")}</span><span className="truncate" title={row.targetName}>{row.targetName ?? "—"}</span><span className="truncate" title={row.referenceName}>{row.referenceName ?? "—"}</span></div>)}{comparison.differencesTruncated && <footer>Showing {comparison.differences.length} of {comparison.differenceCount} differences.</footer>}</> : <div className="empty-note">Every unique root ID and name exists in both files.</div>}</section>
              </div>
              <section className="task-field-candidates">
                <header><div><b>Candidate fixed-width fields</b><span>Tests known field boundaries and ranks positions where skipping a small block makes the newer bytes realign with the older schema.</span></div><button className="btn" onClick={() => void findFieldCandidates()} disabled={candidatesBusy || layoutBusy || scoringBlocked}>{candidatesBusy ? <Loader2 size={14} className="spin"/> : <FlaskConical size={14}/>} {candidatesBusy ? "Scoring candidates…" : scoringBlocked ? "Structural operation added" : layoutPatch ? "Find next candidates" : fieldCandidates ? "Run again" : "Find candidates"}</button></header>
                {layoutPatch && !fieldCandidates && <div className="task-field-candidates-note"><b>Ready for the next field</b><span>The scorer will remove the {layoutPatch.operations.length} accepted operation{layoutPatch.operations.length === 1 ? "" : "s"} from temporary root copies and rank what still differs.</span></div>}
                {fieldCandidates && <>{fieldCandidates.candidates.length ? <>
                  <div className="task-field-candidates-note"><b>{fieldCandidates.candidateCount} candidate{fieldCandidates.candidateCount === 1 ? "" : "s"}</b><span>from {count(fieldCandidates.sampledRoots)} changed matching roots · baseline v{fieldCandidates.baselineVersion}</span><span>Adding one stores a typed fixed-width patch and rechecks every root.</span></div>
                  <div className="task-field-candidates-head"><span>Score</span><span>Insert after</span><span>Width</span><span>Likely types</span><span>Evidence</span><span>Offsets</span><span>Example bytes</span><span/></div>
                  {fieldCandidates.candidates.map((candidate, index) => <div className="task-field-candidate" key={`${candidate.structure}:${candidate.afterField}:${candidate.width}:${index}`}><b>{candidate.score.toFixed(1)}%</b><span className="mono truncate" title={`${candidate.structure}.${candidate.afterField}`}>{candidate.structure}.{candidate.afterField}</span><span>{candidate.width} B</span><span>{candidate.typeHints.join(" · ")}</span><span>{candidate.supportingSamples} / {candidate.testedSamples}</span><span className="mono">0x{candidate.minOffset.toString(16).toUpperCase()}{candidate.maxOffset !== candidate.minOffset ? `–0x${candidate.maxOffset.toString(16).toUpperCase()}` : ""}</span><span className="mono truncate" title={candidate.exampleValues.join(" | ")}>{candidate.exampleValues.join(" | ") || "—"}</span><button className="btn small" onClick={() => setCandidateDraft({ candidate, name: `unknown_v${unsupported.version}_${(layoutPatch?.operations.length ?? 0) + 1}`, fieldType: safestCandidateType(candidate) })} disabled={layoutBusy}>Add</button></div>)}
                  {fieldCandidates.candidatesTruncated && <footer>Showing the best {fieldCandidates.candidates.length} of {fieldCandidates.candidateCount} candidates.</footer>}
                </> : <div className="empty-note">No fixed-width insertion had enough repeatable byte-alignment evidence. The change may be conditional, variable-length, or outside the tested 1–32 byte widths.</div>}</>}
                {candidateDraft && <div className="task-candidate-draft"><span>Insert <b>{candidateDraft.candidate.width} bytes</b> after <span className="mono">{candidateDraft.candidate.structure}.{candidateDraft.candidate.afterField}</span></span><label>Field name <input value={candidateDraft.name} onChange={(event) => setCandidateDraft({ ...candidateDraft, name: event.target.value })} autoFocus /></label><label>Type <select value={candidateDraft.fieldType} onChange={(event) => setCandidateDraft({ ...candidateDraft, fieldType: event.target.value })}>{fixedTypeOptions(candidateDraft.candidate.width).map((type) => <option value={type} key={type}>{type}</option>)}</select></label><button className="btn primary small" onClick={() => void addCandidate()} disabled={layoutBusy}>{layoutBusy ? <Loader2 size={14} className="spin"/> : <Check size={14}/>} Add and analyze</button><button className="btn small" onClick={() => setCandidateDraft(null)} disabled={layoutBusy}>Cancel</button></div>}
              </section>
            </>}
          </section>
        </div>
      </section>{schemaOpen && <TaskSchemaDialog version={unsupported.version} baselineVersion={layoutPatch?.baseVersion ?? baseline} onClose={() => setSchemaOpen(false)}/>} {arrayOpen && <TaskCountedArrayDialog version={unsupported.version} baselineVersion={layoutPatch?.baseVersion ?? baseline} operationNumber={(layoutPatch?.operations.length ?? 0) + 1} onApply={addCountedArray} onClose={() => setArrayOpen(false)}/>} {baselineFieldOpen && <TaskBaselineFieldDialog version={unsupported.version} baselineVersion={layoutPatch?.baseVersion ?? baseline} onApply={changeBaselineField} onClose={() => setBaselineFieldOpen(false)}/>}</>;
    }
    return <section className="tasks-pane empty">
      <div className="drop-card tasks-empty">
        <ListTree size={34} />
        <h2>Open a tasks.data file</h2>
        <p className="muted">JD IDE verifies the index and every numbered task pack before showing the root-task list. Versions 165, 172 and 184 are supported.</p>
        <button className="btn primary" onClick={choose} disabled={busy}><FolderOpen size={15} /> {busy ? "Verifying task packs…" : "Choose tasks.data…"}</button>
        {defaultPath && <button className="btn" onClick={() => void load(defaultPath)} disabled={busy} title={defaultPath}>Open configured client tasks</button>}
        {error && <div className="path-data-message error">{error}</div>}
      </div>
    </section>;
  }

  return <><section className="tasks-pane" aria-busy={busy || taskBusy}>
    <header className="tasks-head">
      <div>
        <h2>Tasks editor</h2>
        <div className="tasks-file-line">
          <span className="mono truncate" title={file.path}>{file.path}</span>
          <span className="path-data-badge"><b>Version:</b> v{file.version}</span>
          <span className="path-data-badge"><b>Roots:</b> {count(file.rootCount)}</span>
          <span className="path-data-badge"><b>Packs:</b> {file.packCount}</span>
          <span className="path-data-badge"><b>Size:</b> {bytes(file.size)}</span>
        </div>
      </div>
      <span className="tasks-integrity" title="The index, pack headers, offsets and every stored pack MD5 were verified"><ShieldCheck size={14} /> Integrity verified</span>
      {file.userLayout && <span className="tag ok" title="This user layout decoded and re-encoded every root byte-for-byte">Accepted user layout</span>}
      {!!editState.changedRoots.length && <span className="status-edits"><span className="changed-dot" /> {editState.changedRoots.length} changed root{editState.changedRoots.length === 1 ? "" : "s"}</span>}
      <button className="btn" onClick={() => setSchemaOpen(true)} disabled={busy || taskBusy}><Braces size={14}/> Task schema</button>
      {file.userLayout && <button className="btn" onClick={() => void reopenLayoutAnalyzer()} disabled={busy || taskBusy || layoutBusy}><Pencil size={14}/> Edit layout</button>}
      <button className="btn" onClick={choose} disabled={busy}><FolderOpen size={14} /> Open…</button>
      <button className="btn primary" onClick={openSave} disabled={busy || taskBusy}><FileCheck2 size={14} /> Save…</button>
    </header>
    {error && <div className="path-data-message error">{error}</div>}
    <div className="tasks-body">
      <aside className="tasks-roots">
        <div className="tasks-search"><Search size={14} /><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search all quests by ID or name…" autoComplete="off" /></div>
        <div className="tasks-root-list">
          {shown.map((match) => {
            const root = match.root;
            if (match.path.length) {
              const key = branchKey(root, match.path);
              const selected = selectedBranch === key;
              return <div className={"task-list-row child task-search-result" + (selected ? " selected" : "")} key={`${rootKey(root)}:${pathKey(match.path)}`}>
                <span className="task-disclosure-spacer task-search-branch">↳</span>
                <button className="task-list-select" onClick={() => void selectTask(root, match.path)} title={`Subquest of ${root.id} · path ${match.path.map((part) => part + 1).join(".")}`}>
                  <span className="mono task-tree-id">{match.id}</span>
                  <span className="truncate">{match.name || "(unnamed task)"}</span>
                  <span className="task-search-context">root {root.id}{match.childCount ? ` · ${match.childCount} subquests` : ""}</span>
                </button>
              </div>;
            }
            const tree = trees.get(rootKey(root));
            const key = branchKey(root, []);
            const open = expanded.has(key);
            const selected = selectedBranch === key;
            return <div className="task-root-branch" key={rootKey(root)}>
              <div className={"task-list-row root" + (selected ? " selected" : "")}>
                {root.childCount ? <button className="task-disclosure" onClick={() => tree ? toggleBranch(root, []) : void selectTask(root, [], true)} title={open ? `Collapse ${root.childCount} subtasks` : `Expand ${root.childCount} subtasks`} aria-label={open ? `Collapse ${root.name}` : `Expand ${root.name}`}>{open ? <Minus size={11} /> : <Plus size={11} />}</button> : <span className="task-disclosure-spacer" />}
                <button className="task-list-select" onClick={() => void selectTask(root, [])} title={`Root task ${root.index + 1} · ID ${root.id}`}>
                  <span className="mono task-tree-id">{root.id}</span>
                  <span className="truncate">{changedRoots.has(rootKey(root)) && <span className="changed-dot" />} {root.name || "(unnamed task)"}</span>
                  <span className="muted mono">{bytes(root.byteSize)}</span>
                </button>
              </div>
              {open && tree?.children.map((child) => <NestedTaskRow key={pathKey(child.path)} root={root} node={child} selected={selectedBranch} expanded={expanded} onSelect={(taskPath) => void selectTask(root, taskPath)} onToggle={(taskPath) => toggleBranch(root, taskPath)} />)}
            </div>;
          })}
          {!shown.length && <div className="empty-note center">{searchReport?.error || (deferredQuery && !searchReport ? "Searching…" : deferredQuery && !searchReport?.indexed ? "Indexing subquests…" : "No quests match this search.")}</div>}
        </div>
        <footer className="tasks-page">
          <span>{matches.length ? page * PAGE_SIZE + 1 : 0}–{Math.min((page + 1) * PAGE_SIZE, matches.length)} of {count(deferredQuery ? searchReport?.total ?? 0 : matches.length)}</span>
          {deferredQuery && searchReport && !searchReport.indexed && !searchReport.error && <span className="tasks-indexing">Indexing subquests…</span>}
          <span className="spacer" />
          <button className="icon-btn small" onClick={() => setPage((value) => Math.max(0, value - 1))} disabled={page === 0} title="Previous page">‹</button>
          <label>Page <input type="number" min={1} max={pages} value={pageInput} onChange={(event) => setPageInput(event.target.value)} onBlur={applyPageInput} onKeyDown={(event) => event.key === "Enter" && applyPageInput()} /> / {pages}</label>
          <button className="icon-btn small" onClick={() => setPage((value) => Math.min(pages - 1, value + 1))} disabled={page + 1 >= pages} title="Next page">›</button>
        </footer>
      </aside>
      <section className="tasks-inspector">
        {detail ? <>
          <header className="tasks-inspector-head">
            <div className="task-title-icon"><FileCheck2 size={18} /></div>
            <div className="truncate"><h2 className="truncate">{detail.name || "(unnamed task)"}</h2><span className="muted mono">ID {detail.id} · root {selectedRoot ? selectedRoot.index + 1 : "?"} · offset 0x{detail.taskOffset.toString(16).toUpperCase()} · {bytes(detail.taskSize)}</span></div>
            {taskBusy && <span className="muted">Reading…</span>}
            <div className="task-edit-actions">
              {selectedPath.length === 0 && <button className="btn small" onClick={() => void cloneSelectedRoot()} disabled={taskBusy || !taskIndexReady} title={taskIndexReady ? "Clone this complete top-level task with fresh task IDs" : "Task IDs are still being indexed"}><Copy size={13}/> {taskIndexReady ? "Clone task" : "Indexing IDs…"}</button>}
              {selectedPath.length > 0 && <button className="btn small" onClick={() => void cloneSelectedSubtree()} disabled={taskBusy || !taskIndexReady} title={taskIndexReady ? "Clone this subquest and all of its descendants beside the source" : "Task IDs are still being indexed"}><Copy size={13}/> {taskIndexReady ? "Clone subtree" : "Indexing IDs…"}</button>}
              {selectedPath.length > 0 && <button className="btn small" onClick={openMoveSelected} disabled={taskBusy || !taskIndexReady} title={taskIndexReady ? "Choose a new parent for this subquest tree" : "Task destinations are still being indexed"}><ArrowRight size={13}/> Move subtree</button>}
              {selectedPath.length > 0 && <button className="btn small danger-outline" onClick={() => void inspectDeleteSelected()} disabled={taskBusy || !taskIndexReady} title={taskIndexReady ? "Review references and delete this subquest with all descendants" : "Task references are still being indexed"}><Trash2 size={13}/> Delete subtree</button>}
              <button className="icon-btn" onClick={undo} disabled={!editState.undo || taskBusy} title={editState.undo ? `Undo ${editState.undo}` : "Nothing to undo"}><Undo2 size={14} /></button>
              <button className="icon-btn" onClick={redo} disabled={!editState.redo || taskBusy} title={editState.redo ? `Redo ${editState.redo}` : "Nothing to redo"}><Redo2 size={14} /></button>
              <button className={"icon-btn" + (history !== null ? " active" : "")} onClick={() => history === null ? void getTaskEditHistory().then(setHistory).catch((problem) => setError(String(problem))) : setHistory(null)} title="Edit history"><History size={14} /></button>
              <button className="icon-btn" onClick={revertAll} disabled={!editState.changedRoots.length || taskBusy} title="Revert all task edits"><RotateCcw size={14} /></button>
            </div>
          </header>
          {history !== null && <section className="task-history">
            <header><b>Edit history</b><span>{history.length} operation{history.length === 1 ? "" : "s"}</span><button className="icon-btn small" onClick={() => setHistory(null)} title="Close history"><X size={13} /></button></header>
            <div>{history.length ? history.slice().reverse().map((entry) => <div className={"task-history-entry" + (entry.undone ? " undone" : "")} key={entry.id}>
              <span className="mono">{new Date(entry.time).toLocaleTimeString()}</span>
              <b>{entry.label}</b>
              <span className="truncate">{entry.taskId ? `${entry.taskId} · ${entry.taskName}` : entry.taskName}</span>
              <span className="truncate" title={`${entry.old} → ${entry.new}`}>{entry.old} → {entry.new}</span>
              {entry.undone && <span className="tag">undone</span>}
            </div>) : <div className="empty-note">No task edits yet.</div>}</div>
          </section>}
          <div className="task-fields-head"><span>Field</span><span>Value</span><span>Type</span><span>Offset</span></div>
          <div className="task-fields">{categories.map((category) => <details className="task-category" key={category.key} open>
            <summary><span>{category.label}</span><span>{category.fields.length} fields</span></summary>
            {category.fields.map((field, index) => <FieldRow key={`${field.name}:${field.offset}:${index}`} field={field} onReference={followReference} onEdit={editField} />)}
          </details>)}</div>
        </> : <div className="empty-note center">Select a task to inspect its fields.</div>}
      </section>
    </div>
    {savedNote && <div className="path-data-message ok">{savedNote}</div>}
    {saving && <TaskSaveDialog path={file.path} onCancel={() => setSaving(false)} onSaved={(report) => {
      setSaving(false);
      setFile((current) => current ? { ...current, path: report.path, size: report.size } : current);
      getTaskEditState().then(setEditState).catch((problem) => setError(String(problem)));
      setSavedNote(`Saved ${report.changedRoots} changed root${report.changedRoots === 1 ? "" : "s"} across ${report.changedPacks} pack${report.changedPacks === 1 ? "" : "s"}.`);
      window.setTimeout(() => setSavedNote(null), 5000);
    }} />}
  </section>{schemaOpen && <TaskSchemaDialog version={file.version} baselineVersion={file.version} onClose={() => setSchemaOpen(false)}/>} {moveSource && <TaskMoveDialog source={moveSource} busy={moveBusy} error={moveError} onConfirm={(destination) => void confirmMoveSelected(destination)} onClose={() => { if (!moveBusy) { setMoveSource(null); setMoveError(null); } }}/>} {deletePreview && <TaskDeleteDialog preview={deletePreview} busy={deleteBusy} error={deleteError} onConfirm={() => void confirmDeleteSelected()} onClose={() => { if (!deleteBusy) { setDeletePreview(null); setDeleteError(null); } }}/>}</>;
});
