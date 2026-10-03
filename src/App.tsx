import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { cloneRecord, deleteRecord, editRecord, fileSummary, getRecord, getSettings, getTalk, iconUrl, listRecords, listTalks, openElements, redoEdit, revertEdits, saveElements, undoEdit } from "./elements/api";
import type { EditState, Saved, SaveOptions, ExportSource, FieldEdit, FileSummary, FindHit, ListSummary, RecordDetail, RecordRow, SettingsView, TalkDetail } from "./elements/types";
import { bytes, count } from "./elements/format";
import { ListPicker, type ListPickerHandle } from "./components/ListPicker";
import { RecordTable } from "./components/RecordTable";
import { type FieldFocus, RecordInspector } from "./components/RecordInspector";
import { AdvancedSearch } from "./components/AdvancedSearch";
import { ProblemsPanel } from "./components/ProblemsPanel";
import { CoveragePanel } from "./components/CoveragePanel";
import { ComparePanel } from "./components/ComparePanel";
import { HistoryPanel } from "./components/HistoryPanel";
import { DeleteDialog } from "./components/DeleteDialog";
import { ExportDialog } from "./components/ExportMenu";
import { ImportRecordsDialog } from "./components/ImportRecordsDialog";
import { type Menu, MenuBar } from "./components/MenuBar";
import { SchemaEditor } from "./components/SchemaEditor";
import { TabBar, type TabLabel } from "./components/TabBar";
import { SettingsDialog } from "./components/SettingsDialog";
import { SetsEditor } from "./components/SetsEditor";
import { FindPalette } from "./components/FindPalette";
import { DialogViewer } from "./components/DialogViewer";
import { SaveDialog } from "./components/SaveDialog";
import { UnsavedDialog } from "./components/UnsavedDialog";
import type { FieldSpec } from "./schema/model";
import { DIALOGS, EMPTY_TABS, type Location, type Tab, loadTabs, makeTab, saveTabs, tabsReducer } from "./tabs";
import "./App.css";
import logo from "./assets/logo.png";
import {
  Braces,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  Database,
  Download,
  FileUp,
  FileStack,
  FolderOpen,
  Gauge,
  Copy,
  Gem,
  GitCompareArrows,
  Trash2,
  History,
  ListFilter,
  Redo2,
  RotateCcw,
  Save,
  SaveAll,
  Search,
  Settings,
  Undo2,
} from "lucide-react";

/** Records changed, added or deleted since the file was opened or saved. */
function editCountOf(e: EditState): number {
  return e.changed.length + e.added.length + e.deleted.reduce((n, [, c]) => n + c, 0);
}

/** What the left side of the workspace shows. */
type Panel = "lists" | "search" | "problems" | "compare" | "coverage" | "history";

/** The last Find: its hits are stepped through with F3 / Shift+F3. */
interface LastFind {
  query: string;
  hits: FindHit[];
  position: number;
}

const LAST_PATH_KEY = "jdide.lastPath";

function readLastPath(): string | null {
  try {
    return localStorage.getItem(LAST_PATH_KEY);
  } catch {
    return null;
  }
}

function writeLastPath(path: string) {
  try {
    localStorage.setItem(LAST_PATH_KEY, path);
  } catch {
    // Remembering the last file is a convenience only.
  }
}

const NO_EDITS: EditState = { changed: [], added: [], deleted: [], shifts: [] };

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** Rows of a list, or of the NPC dialogs (named by their first words). */
async function loadRows(list: number): Promise<RecordRow[]> {
  if (list !== DIALOGS) return listRecords(list);
  const talks = await listTalks();
  return talks.map((t) => ({ index: t.index, id: t.id, name: t.title }));
}

/** The NPC dialogs, shown in the record table like a list. */
function dialogsList(summary: FileSummary): ListSummary {
  return {
    index: DIALOGS,
    name: "NPC Dialogs",
    key: null,
    structName: "TALK_PROC",
    itemSize: 0,
    count: summary.talkCount,
    offset: 0,
    layout: "exact",
    layoutId: null,
    layoutSize: null,
    custom: false,
  };
}

function parseLabel(summary: FileSummary): { text: string; tone: "ok" | "warn" | "muted"; title: string } {
  switch (summary.parseMode) {
    case "layout":
      return {
        text: `Layout ${summary.layoutId}`,
        tone: summary.layoutUnverified ? "warn" : "ok",
        title:
          (summary.layoutSource ?? "") +
          (summary.layoutUnverified ? "\nThe list count of this layout was never checked against a real file." : ""),
      };
    case "markers":
      return {
        text: `Markers from ${summary.markersFrom}`,
        tone: "warn",
        title: `No layout exists for v${summary.version}. Lists were split using the ${summary.markersFrom} marker table, and names and fields are borrowed by record size.`,
      };
    case "detected":
      return {
        text: "Detected",
        tone: "muted",
        title: "No known marker table fits this file. Lists were found by recognising each segment, and names and fields are borrowed by record size.",
      };
  }
}

export default function App() {
  const [summary, setSummary] = useState<FileSummary | null>(null);
  const [fileKey, setFileKey] = useState(0);
  const [loading, setLoading] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [rows, setRows] = useState<RecordRow[] | null>(null);
  const [detail, setDetail] = useState<RecordDetail | null>(null);
  const [talk, setTalk] = useState<TalkDetail | null>(null);
  // Edits in memory (until saving): undo/redo labels and the changed records.
  const [edits, setEdits] = useState<EditState>(NO_EDITS);
  // Bumped after edits, so rows and the open record are read again.
  const [dataVersion, setDataVersion] = useState(0);
  const [lastPath, setLastPath] = useState<string | null>(readLastPath);
  const rowsCache = useRef(new Map<number, RecordRow[]>());
  // Bumped when rows load in the background, so tab titles can use names.
  const [, setRowsLoaded] = useState(0);
  const [tabs, dispatch] = useReducer(tabsReducer, EMPTY_TABS);
  const [editorOpen, setEditorOpen] = useState(false);
  const [editorIntent, setEditorIntent] = useState<{ list: number; offset: number; spec: FieldSpec } | null>(null);
  const [settingsView, setSettingsView] = useState<SettingsView | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  // The enums & masks editor, open at a set (or none).
  const [setsEditor, setSetsEditor] = useState<{ key: string | null } | null>(null);
  const started = useRef(false);
  const iconGen = settingsView?.client?.hasItemIcons ? settingsView.iconGeneration : null;
  const [findOpen, setFindOpen] = useState(false);
  const [panel, setPanel] = useState<Panel>("lists");
  // Panels stay mounted once opened, so their state (results, scans) stays.
  const [mounted, setMounted] = useState<Set<Panel>>(new Set(["lists"]));
  const [problemCounts, setProblemCounts] = useState<{ errors: number; warnings: number } | null>(null);
  useEffect(() => {
    setMounted(new Set(["lists"]));
    setPanel("lists");
    setProblemCounts(null);
  }, [fileKey]);
  /** Shows a panel; showing the open one again goes back to the lists. */
  const togglePanel = (p: Panel) => {
    setMounted((m) => (m.has(p) ? m : new Set([...m, p])));
    setPanel((current) => (current === p && p !== "lists" ? "lists" : p));
  };
  const showPanel = (p: Panel) => {
    setMounted((m) => (m.has(p) ? m : new Set([...m, p])));
    setPanel(p);
  };
  // Tools › Export: what to export, in a dialog.
  const [exporting, setExporting] = useState<{ source: ExportSource; name: string; title: string } | null>(null);
  const [importing, setImporting] = useState(false);
  const [focus, setFocus] = useState<FieldFocus | null>(null);
  const [lastFind, setLastFind] = useState<LastFind | null>(null);
  // Hits belong to one file.
  useEffect(() => setLastFind(null), [fileKey]);

  const icon = (pathId?: number | null) => (iconGen !== null && pathId ? iconUrl(iconGen, pathId) : undefined);

  // The active tab decides what the sidebar, the table and the inspector show.
  const activeTab = tabs.tabs.find((t) => t.id === tabs.active) ?? null;
  const listIndex = activeTab?.list ?? null;
  const recordIndex = activeTab?.row ?? null;

  const editsRef = useRef(edits);
  editsRef.current = edits;
  // Asking what to do with unsaved edits before an action.
  const [unsaved, setUnsaved] = useState<{ action: string; proceed: () => void } | null>(null);
  /** Runs `proceed` now, or after asking when there are unsaved edits. */
  const guardUnsaved = useCallback((action: string, proceed: () => void) => {
    if (editCountOf(editsRef.current) === 0) proceed();
    else setUnsaved({ action, proceed });
  }, []);
  const readFile = useCallback(async (path: string) => {
    setLoading(path);
    setError(null);
    try {
      const result = await openElements(path);
      setEdits(NO_EDITS);
      setFileKey((k) => k + 1);
      lastSave.current = null;
      rowsCache.current.clear();
      setSummary(result);
      setRows(null);
      setDetail(null);
      const first = result.lists.findIndex((l) => l.count > 0);
      const restored = loadTabs(path, result.lists.map((l) => l.count), result.talkCount);
      dispatch({
        type: "reset",
        state:
          restored ??
          (first < 0 ? EMPTY_TABS : (() => {
            const tab = makeTab({ list: first, row: null }, false);
            return { tabs: [tab], active: tab.id };
          })()),
      });
      writeLastPath(path);
      setLastPath(path);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(null);
    }
  }, []);
  const loadFile = useCallback((path: string) => guardUnsaved("open another file", () => readFile(path)), [guardUnsaved, readFile]);

  // Saving: the dialog (to a path, then maybe an action waiting for the
  // save), and the choices of the last save, so Ctrl+S saves right away.
  const [saveDialog, setSaveDialog] = useState<{ path: string; then?: () => void } | null>(null);
  const lastSave = useRef<SaveOptions | null>(null);
  const [savedNote, setSavedNote] = useState<string | null>(null);
  const savedNoteTimer = useRef<number | undefined>(undefined);

  // Closing the window asks about unsaved edits.
  useEffect(() => {
    const win = getCurrentWindow();
    const unlisten = win.onCloseRequested((event) => {
      if (editCountOf(editsRef.current) === 0) return;
      event.preventDefault();
      setUnsaved({ action: "close JD IDE", proceed: () => void win.destroy() });
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // Settings, and the client's elements.data on start when asked for.
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    getSettings()
      .then((view) => {
        setSettingsView(view);
        const elements = view.client?.elementsPath;
        if (view.settings.openOnStart && elements) loadFile(elements);
      })
      .catch((e) => setError(String(e)));
  }, [loadFile]);

  // New client settings: reload rows and the record so icons and paths appear.
  const onSettingsSaved = (view: SettingsView) => {
    setSettingsView(view);
    rowsCache.current.clear();
    setSummary((s) => (s ? { ...s } : s));
  };

  // Remember the open tabs per file.
  useEffect(() => {
    if (summary) saveTabs(summary.path, tabs);
  }, [summary, tabs]);

  const chooseFile = useCallback(async () => {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [
        { name: "Element data", extensions: ["data"] },
        { name: "All files", extensions: ["*"] },
      ],
    });
    if (typeof picked === "string") await loadFile(picked);
  }, [loadFile]);

  // Ctrl+O and drag-and-drop open a file.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector('.import-records-dialog')) return;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "o") {
        e.preventDefault();
        chooseFile();
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        if (e.shiftKey) saveAsRef.current();
        else saveRef.current();
      }
    };
    window.addEventListener("keydown", onKey);
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      if (document.querySelector('.import-records-dialog')) return;
      if (event.payload.type === "drop" && event.payload.paths.length) loadFile(event.payload.paths[0]);
    });
    return () => {
      window.removeEventListener("keydown", onKey);
      unlisten.then((f) => f());
    };
  }, [chooseFile, loadFile]);

  // Load the records of the active tab's list.
  useEffect(() => {
    if (listIndex === null || (listIndex < 0 && listIndex !== DIALOGS)) {
      setRows(null);
      return;
    }
    const show = (result: RecordRow[]) => {
      setRows(result);
      dispatch({ type: "settle", row: result.length ? 0 : null });
    };
    const cached = rowsCache.current.get(listIndex);
    if (cached) {
      show(cached);
      return;
    }
    setRows(null);
    let cancelled = false;
    loadRows(listIndex)
      .then((result) => {
        rowsCache.current.set(listIndex, result);
        if (!cancelled) show(result);
      })
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, [listIndex, summary]);

  // Load the active record.
  useEffect(() => {
    if (listIndex === null || recordIndex === null) {
      setDetail(null);
      setTalk(null);
      return;
    }
    let cancelled = false;
    if (listIndex === DIALOGS) {
      setDetail(null);
      getTalk(recordIndex)
        .then((result) => !cancelled && setTalk(result))
        .catch((e) => !cancelled && setError(String(e)));
    } else {
      setTalk(null);
      getRecord(listIndex, recordIndex)
        .then((result) => !cancelled && setDetail(result))
        .catch((e) => !cancelled && setError(String(e)));
    }
    return () => {
      cancelled = true;
    };
  }, [listIndex, recordIndex, summary, dataVersion]);

  // Load record names for the other tabs' lists, for their titles.
  useEffect(() => {
    if (!summary) return;
    const missing = [...new Set(tabs.tabs.map((t) => t.list))].filter((l) => !rowsCache.current.has(l));
    let cancelled = false;
    (async () => {
      for (const list of missing) {
        const result = await loadRows(list).catch(() => null);
        if (cancelled || !result) return;
        rowsCache.current.set(list, result);
        setRowsLoaded((n) => n + 1);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [tabs.tabs, summary, dataVersion]);

  const openLocation = (loc: Location, options: { pin?: boolean; newTab?: boolean } = {}) =>
    dispatch({ type: "open", loc, ...options });

  const selectList = (index: number) => openLocation({ list: index, row: null });

  const follow = (list: number, row: number, newTab = false) =>
    newTab ? openLocation({ list, row }, { newTab: true }) : dispatch({ type: "navigate", loc: { list, row } });

  // A schema was saved: the file was re-read with the new layouts. The tabs
  // keep their records while everything reloads.
  const onSchemaSaved = (next: FileSummary) => {
    rowsCache.current.clear();
    setSummary(next);
  };

  const closeEditor = (list: number) => {
    setEditorOpen(false);
    setEditorIntent(null);
    if (list !== listIndex) selectList(list);
  };

  /** Opens the next (or previous) hit of the last Find. */
  const stepFind = (delta: number) => {
    if (!lastFind?.hits.length) return setFindOpen(true);
    const n = lastFind.hits.length;
    const position = (lastFind.position + delta + n) % n;
    const hit = lastFind.hits[position];
    setLastFind({ ...lastFind, position });
    follow(hit.list, hit.index);
  };
  const stepFindRef = useRef(stepFind);
  stepFindRef.current = stepFind;
  const pickerRef = useRef<ListPickerHandle>(null);
  /**
   * Reads a list's rows again after an edit (names and IDs may have
   * changed), replacing the shown ones in place so the table keeps its place.
   */
  const refreshRows = (list: number) =>
    loadRows(list)
      .then((result) => {
        rowsCache.current.set(list, result);
        if (listIndexRef.current === list) setRows(result);
        setRowsLoaded((n) => n + 1);
      })
      .catch(() => rowsCache.current.delete(list));
  const listIndexRef = useRef(listIndex);
  listIndexRef.current = listIndex;

  /** Sets fields of a record; resolves to an error message, or null. */
  const commitEdit = async (list: number, row: number, fields: FieldEdit[], label: string) => {
    try {
      setEdits(await editRecord(list, row, fields, label));
      refreshRows(list);
      setDataVersion((v) => v + 1);
      return null;
    } catch (e) {
      return String(e);
    }
  };
  /**
   * Rows moved (a clone or a delete, or their undo): tabs follow their
   * records and the list sizes are read again.
   */
  const applyShifts = async (next: EditState) => {
    if (!next.shifts.length) return;
    for (const sh of next.shifts) dispatch({ type: "shiftRows", ...sh });
    setSummary(await fileSummary());
  };
  /** Undo, redo or revert: any record may change, so the lists in view are read again. */
  const afterEdits = (next: EditState) => {
    setEdits(next);
    applyShifts(next);
    const cached = [...rowsCache.current.keys()];
    rowsCache.current.clear();
    for (const list of cached) refreshRows(list);
    setDataVersion((v) => v + 1);
  };
  const undo = () => undoEdit().then(afterEdits).catch((e) => setError(String(e)));
  const redo = () => redoEdit().then(afterEdits).catch((e) => setError(String(e)));
  const revertRecord = () =>
    listIndex !== null && recordIndex !== null && revertEdits([[listIndex, recordIndex]], "Revert record").then(afterEdits).catch((e) => setError(String(e)));
  const revertAll = () => {
    if (!window.confirm(`Put the file back as it was ${edits.lastSaved ? "last saved" : "opened"} (${editCount} record(s) changed, added or deleted)? Undo brings the edits back.`)) return;
    revertEdits(null, "Revert all changes").then(afterEdits).catch((e) => setError(String(e)));
  };
  /** A save went through: the open file now is the saved one. */
  const onSaved = (saved: Saved, options: SaveOptions) => {
    const then = saveDialog?.then;
    setSaveDialog(null);
    lastSave.current = { ...options, replaceChanged: false };
    setSummary(saved.summary);
    setEdits(saved.state);
    setDataVersion((v) => v + 1);
    writeLastPath(saved.report.path);
    setLastPath(saved.report.path);
    window.clearTimeout(savedNoteTimer.current);
    setSavedNote(
      saved.report.digest
        ? `Saved at ${new Date(saved.report.timestamp * 1000).toLocaleTimeString()} · checksum ${saved.report.digest.slice(0, 8)}…`
        : `Saved at ${new Date(saved.report.timestamp * 1000).toLocaleTimeString()} · checksum not updated (no path.data)`,
    );
    savedNoteTimer.current = window.setTimeout(() => setSavedNote(null), 6000);
    then?.();
  };
  /** Ctrl+S: saves right away with the last choices, else asks first. */
  const saveFile = async () => {
    if (!summary) return;
    const last = lastSave.current;
    if (!last || last.path !== summary.path) return setSaveDialog({ path: summary.path });
    try {
      onSaved(await saveElements(last), last);
    } catch (e) {
      // Changed on disk, or failed: the dialog shows why.
      setSaveDialog({ path: summary.path });
      if (!String(e).includes("CHANGED_ON_DISK")) setError(String(e));
    }
  };
  const saveFileAs = () => summary && setSaveDialog({ path: summary.path });
  const saveRef = useRef(saveFile);
  saveRef.current = saveFile;
  const saveAsRef = useRef(saveFileAs);
  saveAsRef.current = saveFileAs;

  // Clone and delete the open record.
  const [deleting, setDeleting] = useState<{ list: number; row: number } | null>(null);
  const cloneOpen = async () => {
    if (listIndex === null || listIndex < 0 || recordIndex === null) return;
    try {
      const next = await cloneRecord(listIndex, recordIndex);
      afterEdits(next);
      if (next.created) openLocation({ list: next.created[0], row: next.created[1] }, { pin: true });
    } catch (e) {
      setError(String(e));
    }
  };
  const confirmDelete = async () => {
    if (!deleting) return;
    const target = deleting;
    setDeleting(null);
    try {
      afterEdits(await deleteRecord(target.list, target.row));
    } catch (e) {
      setError(String(e));
    }
  };
  const cloneRef = useRef(cloneOpen);
  cloneRef.current = cloneOpen;
  const undoRef = useRef(undo);
  undoRef.current = undo;
  const redoRef = useRef(redo);
  redoRef.current = redo;
  const togglePanelRef = useRef(togglePanel);
  togglePanelRef.current = togglePanel;

  // Tab, history and find shortcuts (not while the schema editor is open).
  useEffect(() => {
    if (!summary || editorOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector('.import-records-dialog')) return;
      const mod = e.ctrlKey || e.metaKey;
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement;
      if (mod && !typing && e.key.toLowerCase() === "d") {
        e.preventDefault();
        cloneRef.current();
      } else if (mod && !typing && e.key.toLowerCase() === "z") {
        e.preventDefault();
        (e.shiftKey ? redoRef : undoRef).current();
      } else if (mod && !typing && e.key.toLowerCase() === "y") {
        e.preventDefault();
        redoRef.current();
      } else if (mod && e.shiftKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        togglePanelRef.current("search");
      } else if (mod && e.shiftKey && e.key.toLowerCase() === "m") {
        e.preventDefault();
        togglePanelRef.current("problems");
      } else if (mod && !e.shiftKey && e.key.toLowerCase() === "h") {
        e.preventDefault();
        togglePanelRef.current("history");
      } else if (mod && !e.shiftKey && e.key.toLowerCase() === "l") {
        e.preventDefault();
        setPanel("lists");
        // The picker mounts with the lists panel.
        setTimeout(() => pickerRef.current?.open());
      } else if (mod && e.key.toLowerCase() === "g") {
        e.preventDefault();
        setFindOpen(true);
      } else if (e.key === "F3") {
        e.preventDefault();
        stepFindRef.current(e.shiftKey ? -1 : 1);
      } else if (e.altKey && e.key === "ArrowLeft") {
        e.preventDefault();
        dispatch({ type: "back" });
      } else if (mod && e.key === "Tab") {
        e.preventDefault();
        dispatch({ type: "cycle", delta: e.shiftKey ? -1 : 1 });
      } else if (mod && e.key.toLowerCase() === "w") {
        e.preventDefault();
        if (tabs.active !== null) dispatch({ type: "close", id: tabs.active });
      } else if (mod && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        dispatch({ type: "activateIndex", index: e.key === "9" ? -1 : Number(e.key) - 1 });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [summary, editorOpen, tabs.active]);

  const tabLabel = (tab: Tab): TabLabel => {
    if (tab.list === DIALOGS) {
      if (tab.row === null) return { title: "NPC Dialogs", subtitle: "dialogs" };
      const row = rowsCache.current.get(DIALOGS)?.[tab.row];
      return { title: row?.name || `Dialog #${tab.row}`, subtitle: `NPC dialog · ID ${row?.id ?? "?"}` };
    }
    const l = summary?.lists[tab.list];
    const listName = l?.name ?? `List ${tab.list}`;
    if (tab.row === null) return { title: listName, subtitle: `list ${tab.list}` };
    const row = rowsCache.current.get(tab.list)?.[tab.row];
    return {
      title: row?.name || `${listName} #${tab.row}`,
      subtitle: `${listName} · #${tab.row}`,
      icon: icon(row?.icon),
      changed: !!changedRows.get(tab.list)?.has(tab.row!) || !!addedRows.get(tab.list)?.has(tab.row!),
    };
  };

  const showingDialogs = listIndex === DIALOGS;
  const changedRows = new Map<number, Set<number>>();
  for (const [l, r] of edits.changed) changedRows.set(l, (changedRows.get(l) ?? new Set()).add(r));
  const addedRows = new Map<number, Set<number>>();
  for (const [l, r] of edits.added) addedRows.set(l, (addedRows.get(l) ?? new Set()).add(r));
  const changedCounts = new Map<number, number>();
  for (const [l, rows] of changedRows) changedCounts.set(l, rows.size);
  for (const [l, rows] of addedRows) changedCounts.set(l, (changedCounts.get(l) ?? 0) + rows.size);
  for (const [l, n] of edits.deleted) changedCounts.set(l, (changedCounts.get(l) ?? 0) + n);
  const editCount = editCountOf(edits);
  const canRecordOp = listIndex !== null && listIndex >= 0 && recordIndex !== null;
  const recordChanged = listIndex !== null && recordIndex !== null && !!changedRows.get(listIndex)?.has(recordIndex);
  const picker = summary && (
    <ListPicker ref={pickerRef} lists={summary.lists} selected={listIndex} onSelect={selectList} talkCount={summary.talkCount} changedCounts={changedCounts} />
  );
  const recordOpen = !!summary && listIndex !== null && listIndex >= 0 && recordIndex !== null;
  const listOpen = !!summary && listIndex !== null && listIndex >= 0;
  const problemBadges = [
    ...(problemCounts?.errors ? [{ text: String(problemCounts.errors), tone: "error" as const }] : []),
    ...(problemCounts?.warnings ? [{ text: String(problemCounts.warnings), tone: "warning" as const }] : []),
  ];
  const noFile = summary ? undefined : "Open a file first";
  const menus: Menu[] = [
    {
      label: "File",
      accessKey: "f",
      items: [
        { label: "Open elements.data…", icon: FolderOpen, shortcut: "Ctrl+O", onSelect: chooseFile },
        {
          label: "Save",
          icon: Save,
          shortcut: "Ctrl+S",
          onSelect: saveFile,
          disabled: !summary,
          badges: editCount ? [{ text: String(editCount), tone: "warning" as const }] : [],
          title: summary ? (editCount ? `Write the ${editCount} changed record(s) to ${fileName(summary.path)}` : "Write the file again (no record changes)") : noFile,
        },
        { label: "Save as…", icon: SaveAll, shortcut: "Ctrl+Shift+S", onSelect: saveFileAs, disabled: !summary, title: summary ? "Write the file to another place" : noFile },
        "separator",
        { label: "Advanced search", icon: ListFilter, shortcut: "Ctrl+Shift+F", onSelect: () => showPanel("search"), disabled: !summary, checked: panel === "search", title: noFile },
        { label: "Problems", badges: problemBadges, icon: CircleAlert, shortcut: "Ctrl+Shift+M", onSelect: () => showPanel("problems"), disabled: !summary, checked: panel === "problems", title: noFile },
        { label: "Compare with another file…", icon: GitCompareArrows, onSelect: () => showPanel("compare"), disabled: !summary, checked: panel === "compare", title: noFile },
        { label: "Layout coverage", icon: Gauge, onSelect: () => showPanel("coverage"), disabled: !summary, checked: panel === "coverage", title: noFile },
        "separator",
        { label: "Settings…", icon: Settings, onSelect: () => setSettingsOpen(true) },
      ],
    },
    {
      label: "Edit",
      accessKey: "e",
      items: [
        { label: edits.undo ? `Undo ${edits.undo}` : "Undo", icon: Undo2, shortcut: "Ctrl+Z", onSelect: undo, disabled: !edits.undo },
        { label: edits.redo ? `Redo ${edits.redo}` : "Redo", icon: Redo2, shortcut: "Ctrl+Y", onSelect: redo, disabled: !edits.redo },
        "separator",
        { label: "Clone record", icon: Copy, shortcut: "Ctrl+D", onSelect: cloneOpen, disabled: !canRecordOp, title: canRecordOp ? "Copy the open record to the end of its list with a new ID" : "Open a record first" },
        {
          label: "Delete record…",
          icon: Trash2,
          shortcut: "Del",
          onSelect: () => canRecordOp && setDeleting({ list: listIndex!, row: recordIndex! }),
          disabled: !canRecordOp,
          title: canRecordOp ? "Delete the open record (asks first, showing what points at it)" : "Open a record first",
        },
        "separator",
        { label: "History", icon: History, shortcut: "Ctrl+H", onSelect: () => showPanel("history"), disabled: !summary, checked: panel === "history", title: noFile },
        "separator",
        { label: "Revert record", icon: RotateCcw, onSelect: revertRecord, disabled: !recordChanged, title: recordChanged ? `Put the open record back as the file was ${edits.lastSaved ? "last saved" : "opened"}` : "The open record has no edits" },
        {
          label: "Revert all changes…",
          onSelect: revertAll,
          disabled: editCount === 0,
          badges: editCount ? [{ text: String(editCount), tone: "warning" as const }] : [],
          title: `Put every changed record back as the file was ${edits.lastSaved ? "last saved" : "opened"} (undoable)`,
        },
      ],
    },
    {
      label: "Tools",
      accessKey: "t",
      items: [
        {
          label: "Export",
          icon: Download,
          disabled: !summary,
          submenu: [
            {
              label: "Selected item…",
              disabled: !recordOpen,
              title: recordOpen ? "The record open in the active tab" : "Open a record first",
              onSelect: () =>
                recordOpen &&
                setExporting({
                  source: { from: "item", list: listIndex!, row: recordIndex! },
                  name: rowsCache.current.get(listIndex!)?.[recordIndex!]?.name || `${summary!.lists[listIndex!].name} ${recordIndex}`,
                  title: `Export ${rowsCache.current.get(listIndex!)?.[recordIndex!]?.name || `record ${recordIndex}`}`,
                }),
            },
            {
              label: "Selected list…",
              disabled: !listOpen,
              title: listOpen ? "Every record of the list open in the active tab" : "Open a list first",
              onSelect: () =>
                listOpen &&
                setExporting({
                  source: { from: "list", list: listIndex! },
                  name: summary!.lists[listIndex!].name,
                  title: `Export ${summary!.lists[listIndex!].name} (${count(summary!.lists[listIndex!].count)} records)`,
                }),
            },
          ],
        },
        { label: "Import JSON…", icon: FileUp, onSelect: () => setImporting(true), disabled: !summary, title: noFile },
      ],
    },
  ];
  const list = summary && listIndex !== null && listIndex >= 0 ? summary.lists[listIndex] : summary && showingDialogs ? dialogsList(summary) : null;
  const row = rows && recordIndex !== null ? (rows[recordIndex] ?? null) : null;

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <img className="logo" src={logo} alt="" draggable={false} />
          JD IDE
        </div>
        <MenuBar menus={menus} />
        {summary && (
          <div className="file-chip" title={summary.path}>
            <span className="truncate">{fileName(summary.path)}</span>
            {editCount > 0 && (
              <button className="dirty-dot" onClick={saveFile} title={`${editCount} record(s) changed since the file was ${edits.lastSaved ? "saved" : "opened"} · Ctrl+S saves`} aria-label="Unsaved changes, save">
                ●
              </button>
            )}
            <span className="tag">v{summary.version}</span>
            {(() => {
              const p = parseLabel(summary);
              return (
                <span className={`tag ${p.tone}`} title={p.title}>
                  {p.text}
                </span>
              );
            })()}
          </div>
        )}
        {summary && (
          <div className="find-bar">
            <button className="find-trigger" onClick={() => setFindOpen(true)} title="Find a record by ID or name in every list (Ctrl+G)">
              <Search size={14} />
              <span className="truncate">{lastFind ? lastFind.query : "Find by ID or name…"}</span>
              <kbd>Ctrl G</kbd>
            </button>
            {lastFind && lastFind.hits.length > 1 && (
              <>
                <span className="find-pos mono muted">
                  {lastFind.position + 1}/{lastFind.hits.length}
                </span>
                <button className="icon-btn small" onClick={() => stepFind(-1)} title="Previous result (Shift+F3)">
                  <ChevronLeft size={15} />
                </button>
                <button className="icon-btn small" onClick={() => stepFind(1)} title="Next result (F3)">
                  <ChevronRight size={15} />
                </button>
              </>
            )}
          </div>
        )}
        <span className="spacer" />
        {loading && <span className="muted">Reading {fileName(loading)}…</span>}
        {savedNote && (
          <span className="saved-note" role="status">
            <Save size={13} /> {savedNote}
          </span>
        )}
        <button
          className={"btn" + (editorOpen ? " active" : "")}
          onClick={() => (editorOpen ? closeEditor(listIndex ?? 0) : setEditorOpen(true))}
          disabled={!summary || !summary.lists.length}
          title={summary ? "Write your own schema for this file's lists" : "Open a file to edit its schema"}
        >
          <Braces size={15} /> Schema editor
        </button>
        <button
          className={"icon-btn topbar-settings" + (settingsOpen ? " active" : "")}
          onClick={() => setSettingsOpen(true)}
          title={
            settingsView?.client
              ? `Settings · client: ${settingsView.client.root}`
              : "Settings: set the game client folder for icons and quick access to its files"
          }
          aria-label="Settings"
        >
          <Settings size={18} />
        </button>
      </header>

      {saveDialog && summary && (
        <SaveDialog path={saveDialog.path} pathData={lastSave.current?.pathData} onCancel={() => setSaveDialog(null)} onSaved={onSaved} />
      )}
      {unsaved && summary && (
        <UnsavedDialog
          action={unsaved.action}
          fileName={fileName(summary.path)}
          changed={edits.changed.length}
          added={edits.added.length}
          deleted={edits.deleted.reduce((n, [, c]) => n + c, 0)}
          onCancel={() => setUnsaved(null)}
          onDiscard={() => {
            const proceed = unsaved.proceed;
            setUnsaved(null);
            proceed();
          }}
          onSave={() => {
            const proceed = unsaved.proceed;
            setUnsaved(null);
            setSaveDialog({ path: summary.path, then: proceed });
          }}
        />
      )}
      {deleting && summary && summary.lists[deleting.list] && (
        <DeleteDialog
          list={summary.lists[deleting.list]}
          row={deleting.row}
          name={rowsCache.current.get(deleting.list)?.[deleting.row]?.name ?? ""}
          id={rowsCache.current.get(deleting.list)?.[deleting.row]?.id ?? 0}
          lists={summary.lists}
          onConfirm={confirmDelete}
          onCancel={() => setDeleting(null)}
        />
      )}

      {exporting && <ExportDialog source={exporting.source} name={exporting.name} title={exporting.title} onClose={() => setExporting(null)} />}
      {importing && summary && <ImportRecordsDialog onApplied={afterEdits} onClose={() => setImporting(false)} />}

      {findOpen && summary && (
        <FindPalette
          lists={summary.lists}
          initialQuery={lastFind?.query ?? ""}
          icon={icon}
          onOpen={(hit, newTab) => follow(hit.list, hit.index, newTab)}
          onResults={(query, hits, position) => setLastFind({ query, hits, position })}
          onClose={() => setFindOpen(false)}
        />
      )}

      {setsEditor && (
        <SetsEditor
          initialKey={setsEditor.key}
          onChanged={(next) => next && onSchemaSaved(next)}
          onClose={() => setSetsEditor(null)}
        />
      )}

      {settingsOpen && settingsView && (
        <SettingsDialog
          view={settingsView}
          onSaved={onSettingsSaved}
          onOpenFile={loadFile}
          onClose={() => setSettingsOpen(false)}
        />
      )}

      {error && (
        <div className="error-bar" role="alert">
          <span>{error}</span>
          <button className="link" onClick={() => setError(null)}>
            Dismiss
          </button>
        </div>
      )}

      {summary && editorOpen ? (
        <main className="editor-main">
          <SchemaEditor
            summary={summary}
            initialList={editorIntent?.list ?? (list && list.index >= 0 ? list.index : 0)}
            intent={editorIntent}
            initialRow={showingDialogs ? 0 : (recordIndex ?? 0)}
            onEditSets={(key) => setSetsEditor({ key })}
            onSaved={onSchemaSaved}
            onClose={closeEditor}
          />
        </main>
      ) : summary ? (
        <main className={"workspace" + (panel !== "lists" ? " searching" : "")}>
          <nav className="activity-bar" aria-label="Data files">
            <button
              className={"activity" + (panel === "lists" ? " active" : " current")}
              onClick={() => setPanel("lists")}
              title={`elements.data: lists and records${panel !== "lists" ? " (back from the tool)" : ""}`}
              aria-label="elements.data"
            >
              <Database size={19} />
            </button>
            <span className="activity-soon" title="More game data files (tasks.data, gshop.data, …) will get their own entry here">
              <FileStack size={17} />
            </span>
          </nav>
          {mounted.has("search") && (
          <div className="search-slot" hidden={panel !== "search"}>
            <AdvancedSearch
              key={fileKey}
              lists={summary.lists}
              currentList={listIndex}
              icon={icon}
              onOpen={(hit, off, newTab) => {
                openLocation({ list: hit.list, row: hit.row }, newTab ? { newTab: true } : {});
                if (off !== null) setFocus({ list: hit.list, row: hit.row, off, nonce: Date.now() });
              }}
              onClose={() => setPanel("lists")}
              onEdited={afterEdits}
            />
          </div>
          )}
          {mounted.has("problems") && (
            <div className="search-slot" hidden={panel !== "problems"}>
              <ProblemsPanel
                key={fileKey}
                lists={summary.lists}
                icon={icon}
                onCounts={(errors, warnings) => setProblemCounts({ errors, warnings })}
                onOpen={(p, newTab) => {
                  const options = newTab ? { newTab: true } : {};
                  if (p.talk !== undefined) openLocation({ list: DIALOGS, row: p.talk }, options);
                  else if (p.list !== undefined && p.row === undefined) openLocation({ list: p.list, row: null }, options);
                  else if (p.list !== undefined && p.row !== undefined) {
                    openLocation({ list: p.list, row: p.row }, options);
                    if (p.off !== undefined) setFocus({ list: p.list, row: p.row, off: p.off, nonce: Date.now() });
                  }
                }}
                onClose={() => setPanel("lists")}
              />
            </div>
          )}
          {mounted.has("compare") && (
            <div className="search-slot" hidden={panel !== "compare"}>
              <ComparePanel
                key={fileKey}
                currentPath={summary.path}
                suggestions={[settingsView?.client?.elementsPath ?? "", lastPath ?? ""]}
                icon={icon}
                onOpen={(list, row, newTab) => openLocation({ list, row }, newTab ? { newTab: true } : {})}
                onClose={() => setPanel("lists")}
              />
            </div>
          )}
          {mounted.has("history") && (
            <div className="search-slot" hidden={panel !== "history"}>
              <HistoryPanel
                key={fileKey}
                lists={summary.lists}
                edits={edits}
                icon={icon}
                onUndo={undo}
                onRedo={redo}
                onChanged={afterEdits}
                onOpen={(list, row, off, newTab) => {
                  openLocation({ list, row }, newTab ? { newTab: true } : {});
                  if (off !== null) setFocus({ list, row, off, nonce: Date.now() });
                }}
                onClose={() => setPanel("lists")}
              />
            </div>
          )}
          {mounted.has("coverage") && (
            <div className="search-slot" hidden={panel !== "coverage"}>
              <CoveragePanel
                generation={summary}
                onOpenList={selectList}
                onEditSchema={(index) => {
                  selectList(index);
                  setEditorIntent(null);
                  setEditorOpen(true);
                }}
                onClose={() => setPanel("lists")}
              />
            </div>
          )}
          {panel !== "lists" ? null : list ? (
            <RecordTable
              list={list}
              rows={rows}
              selected={recordIndex}
              onSelect={(index) => openLocation({ list: list.index, row: index })}
              onOpen={(index) => openLocation({ list: list.index, row: index }, { pin: true })}
              icon={icon}
              meta={showingDialogs ? `${count(summary.talkCount)} dialogs` : undefined}
              picker={picker}
              changed={listIndex !== null ? changedRows.get(listIndex) : undefined}
              added={listIndex !== null ? addedRows.get(listIndex) : undefined}
              onDelete={showingDialogs ? undefined : (index) => setDeleting({ list: list.index, row: index })}
            />
          ) : (
            <section className="pane records">
              <div className="records-picker">{picker}</div>
              <div className="empty-note center">Pick a list to see its records.</div>
            </section>
          )}
          <div className="inspector-column">
            <TabBar
              tabs={tabs.tabs}
              active={tabs.active}
              label={tabLabel}
              onActivate={(id) => dispatch({ type: "activate", id })}
              onPin={(id) => dispatch({ type: "pin", id })}
              onClose={(id) => dispatch({ type: "close", id })}
            />
            {showingDialogs ? (
              <DialogViewer
                detail={talk}
                lists={summary.lists}
                canGoBack={(activeTab?.history.length ?? 0) > 0}
                onBack={() => dispatch({ type: "back" })}
                onFollow={follow}
              />
            ) : list ? (
              <RecordInspector
                list={list}
                row={row}
                detail={detail}
                canGoBack={(activeTab?.history.length ?? 0) > 0}
                onBack={() => dispatch({ type: "back" })}
                onFollow={follow}
                onDefine={(list, offset, spec) => {
                  setEditorIntent({ list, offset, spec });
                  setEditorOpen(true);
                }}
                icon={icon}
                onEditSet={(key) => setSetsEditor({ key })}
                lists={summary.lists}
                focus={focus}
                onEdit={commitEdit}
                onClone={cloneOpen}
                onDelete={() => recordIndex !== null && setDeleting({ list: list.index, row: recordIndex })}
              />
            ) : (
              <section className="pane inspector">
                <div className="empty-note center">No record open. Pick a list, then a record.</div>
              </section>
            )}
          </div>
        </main>
      ) : (
        <main className="welcome">
          <div className="drop-card">
            <img className="drop-logo" src={logo} alt="" draggable={false} />
            <h1>Open an elements.data file</h1>
            <p className="muted">
              Drop a file anywhere in this window, or choose one. Layouts are built in for versions 66, 112, 156,
              158, 160, 165 and 176. Other versions still open: lists are matched to known ones by record size.
            </p>
            <div className="drop-actions">
              {settingsView?.client?.elementsPath && (
                <button
                  className="btn primary"
                  onClick={() => loadFile(settingsView.client!.elementsPath!)}
                  title={settingsView.client.elementsPath}
                >
                  <Gem size={15} /> Open client elements.data
                </button>
              )}
              <button className={"btn" + (settingsView?.client?.elementsPath ? "" : " primary")} onClick={chooseFile}>
                <FolderOpen size={15} /> Choose file…
              </button>
              {lastPath && (
                <button className="btn" onClick={() => loadFile(lastPath)} title={lastPath}>
                  Reopen {fileName(lastPath)}
                </button>
              )}
            </div>
            {lastPath && <div className="muted small truncate">{lastPath}</div>}
            {settingsView && !settingsView.client && (
              <button className="link" onClick={() => setSettingsOpen(true)}>
                <Settings size={13} /> Set your game client folder to open its files quickly and show item icons
              </button>
            )}
          </div>
        </main>
      )}

      <footer className="statusbar">
        {summary ? (
          <>
            <span>{bytes(summary.fileSize)}</span>
            <span>{summary.lists.length} lists</span>
            <span>{count(summary.lists.reduce((n, l) => n + l.count, 0))} records</span>
            <span>{count(summary.talkCount)} NPC dialogs</span>
            <span title="Export time stored in the file header">
              Exported {new Date(summary.timestamp * 1000).toLocaleString()}
            </span>
            {summary.exporter && <span title="Exporter machine name stored in the file">by {summary.exporter}</span>}
            {editCount > 0 && (
              <button className="status-edits" onClick={() => showPanel("history")} title="Show the edit history (Ctrl+H). Edits are kept in memory until saved.">
                <span className="changed-dot" />
                {[
                  edits.changed.length && `${edits.changed.length} changed`,
                  edits.added.length && `${edits.added.length} added`,
                  edits.deleted.length && `${edits.deleted.reduce((n, [, c]) => n + c, 0)} deleted`,
                ]
                  .filter(Boolean)
                  .join(" · ")}{" "}
                · not saved
              </button>
            )}
            <span className="spacer" />
            {problemCounts && (
              <button className={"status-problems" + (problemCounts.errors ? " error" : problemCounts.warnings ? " warning" : "")} onClick={() => showPanel("problems")} title="Show the problems (Ctrl+Shift+M)">
                <CircleAlert size={12} /> {problemCounts.errors} · {problemCounts.warnings}
              </button>
            )}
            {tabs.tabs.length > 0 && (
              <span title="Ctrl+Tab switches tabs, Ctrl+W closes, Ctrl+1…9 jumps">
                {tabs.tabs.length} tab{tabs.tabs.length > 1 ? "s" : ""}
              </span>
            )}
            <span className="mono" title="Raw version word">
              0x{summary.rawVersion.toString(16).toUpperCase()}
            </span>
          </>
        ) : (
          <span>Ready</span>
        )}
      </footer>
    </div>
  );
}
