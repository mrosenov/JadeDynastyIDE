import { forwardRef, useCallback, useDeferredValue, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, Box, CircleAlert, FolderOpen, Info, Loader2, Plus, Redo2, Save, Search, Sparkles, Trash2, TriangleAlert, Undo2, X } from "lucide-react";
import { dynObjectProblems, exportDynObjectsJson, importDynObjectsJson, openDynObjects, saveDynObjects } from "../elements/api";
import { bytes, count } from "../elements/format";
import type { DynObjectCheck, DynObjectInfo, PathDataFile, PathDataRow } from "../elements/types";
import { DynFilePicker } from "./DynFilePicker";
import { DynObjectsCompare } from "./DynObjectsCompare";

interface Row extends PathDataRow {
  key: number;
}

interface History {
  states: Row[][];
  cursor: number;
  /** Key of the edit that produced states[n + 1], for typing coalescing. */
  keys: string[];
}

export interface DynObjectsEditorState {
  loaded: boolean;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  path: string | null;
  objects: number;
}

export interface DynObjectsEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
  save: () => void;
  saveAs: () => void;
  exportJson: () => void;
  importJson: () => void;
  undo: () => void;
  redo: () => void;
  add: () => void;
  deleteSelected: () => void;
  openProblems: () => void;
  compare: () => void;
}

interface Props {
  defaultPath?: string | null;
  active: boolean;
  /** The npcgen.data open in its editor (its objects are counted). */
  npcgenPath: string | null;
  onStateChange: (state: DynObjectsEditorState) => void;
}

type Filter = "all" | "model" | "effect" | "problems" | "unused";
const PAGE_SIZE = 200;
const BACKUP_KEY = "jdide.dynObjects.backup";
const KIND_LABELS: Record<DynObjectInfo["kind"], string> = { model: "model", effect: "effect", empty: "none", other: "not loaded" };

const fileName = (path: string) => path.split(/[\\/]/).pop() || path;
const pathKey = (path: string) => path.replaceAll("/", "\\").toLowerCase();
const messageOf = (problem: unknown) => String(problem).replace(/^Error: /, "");
const kindOf = (path: string): DynObjectInfo["kind"] => {
  const lower = path.toLowerCase();
  return !path.trim() ? "empty" : lower.endsWith(".ecm") ? "model" : lower.endsWith(".gfx") ? "effect" : "other";
};

/** DynamicObjects.data: the client's models and effects of npcgen.data's dynamic objects. */
export const DynObjectsEditor = forwardRef<DynObjectsEditorHandle, Props>(function DynObjectsEditor({ defaultPath, active, npcgenPath, onStateChange }, ref) {
  const [file, setFile] = useState<PathDataFile | null>(null);
  const [history, setHistory] = useState<History>({ states: [], cursor: -1, keys: [] });
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query.trim().toLowerCase());
  const [filter, setFilter] = useState<Filter>("all");
  const [page, setPage] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [backup, setBackup] = useState(() => { try { return localStorage.getItem(BACKUP_KEY) !== "0"; } catch { return true; } });
  const [selected, setSelected] = useState<number | null>(null);
  const [check, setCheck] = useState<DynObjectCheck | null>(null);
  const [checking, setChecking] = useState(false);
  const [problemsOpen, setProblemsOpen] = useState(false);
  const [other, setOther] = useState<{ path: string; rows: PathDataRow[] } | null>(null);
  /** The row whose file is being chosen (null: a new object). */
  const [picking, setPicking] = useState<{ key: number | null; kind: "model" | "effect"; current?: string } | null>(null);
  const savedRows = useRef<Row[] | null>(null);
  const savedByKey = useRef(new Map<number, Row>());
  const nextKey = useRef(1);
  const autoOpened = useRef<string | null>(null);
  const backedUp = useRef(new Set<string>());
  const rows = history.cursor >= 0 ? history.states[history.cursor] : [];
  const dirty = !!file && rows !== savedRows.current;

  const load = useCallback(async (path: string) => {
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const opened = await openDynObjects(path);
      const loaded = opened.rows.map((row) => ({ ...row, key: nextKey.current++ }));
      savedRows.current = loaded;
      savedByKey.current = new Map(loaded.map((row) => [row.key, row]));
      setFile(opened);
      setHistory({ states: [loaded], cursor: 0, keys: [] });
      setQuery("");
      setPage(0);
      setSelected(loaded[0]?.key ?? null);
    } catch (problem) {
      setError(messageOf(problem));
    } finally {
      setBusy(false);
    }
  }, []);
  useEffect(() => {
    if (!defaultPath || file || autoOpened.current === defaultPath) return;
    autoOpened.current = defaultPath;
    void load(defaultPath);
  }, [defaultPath, file, load]);
  const guard = useCallback((action: () => void) => {
    if (!dirty || window.confirm("Discard the unsaved DynamicObjects.data changes?")) action();
  }, [dirty]);
  const choose = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, defaultPath: file?.path || defaultPath || undefined, title: "Open DynamicObjects.data", filters: [{ name: "DynamicObjects.data", extensions: ["data"] }] });
    if (typeof picked === "string") guard(() => void load(picked));
  }, [defaultPath, file?.path, guard, load]);

  // Kinds, client files, problems and npcgen.data uses, shortly after the rows change.
  useEffect(() => {
    if (!file) return;
    let cancelled = false;
    setChecking(true);
    const timer = window.setTimeout(() => {
      dynObjectProblems(rows.map(({ id, path }) => ({ id, path })))
        .then((next) => { if (!cancelled) setCheck(next); })
        .catch((problem) => { if (!cancelled) setError(messageOf(problem)); })
        .finally(() => { if (!cancelled) setChecking(false); });
    }, 350);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [file, npcgenPath, rows]);
  const infoByKey = useMemo(() => {
    const out = new Map<number, DynObjectInfo>();
    // The check answers for the rows in order; a stale answer is ignored row by row.
    if (check && check.infos.length === rows.length) rows.forEach((row, index) => { if (check.infos[index].id === row.id) out.set(row.key, check.infos[index]); });
    return out;
  }, [check, rows]);
  const usesOf = (id: number) => check?.uses[String(id)] ?? 0;
  const problemIds = useMemo(() => new Set((check?.problems ?? []).map((problem) => problem.id)), [check]);

  const commit = useCallback((next: Row[], key: string) => {
    setHistory((current) => {
      if (current.cursor < 0) return current;
      const states = current.states.slice(0, current.cursor + 1);
      const keys = current.keys.slice(0, current.cursor);
      if (current.cursor > 0 && keys[current.cursor - 1] === key) {
        states[current.cursor] = next;
        return { states, cursor: current.cursor, keys };
      }
      states.push(next);
      keys.push(key);
      if (states.length > 201) {
        states.shift();
        keys.shift();
        return { states, cursor: states.length - 1, keys };
      }
      return { states, cursor: current.cursor + 1, keys };
    });
    setNote(null);
  }, []);
  const change = (key: number, field: "id" | "path", value: string) => {
    commit(rows.map((row) => row.key === key ? { ...row, [field]: field === "id" ? Math.max(0, Math.min(0xffff_ffff, Number(value) || 0)) : value } : row), `field:${key}:${field}`);
  };
  /** A new object: the next free ID with a file chosen from the client. */
  const addWith = (path: string) => {
    const highest = rows.reduce((max, row) => Math.max(max, row.id), 0);
    const row: Row = { key: nextKey.current++, id: highest + 1, path };
    commit([...rows, row], `add:${row.key}`);
    setQuery("");
    setFilter("all");
    setSelected(row.key);
    setPage(Math.floor(rows.length / PAGE_SIZE));
    setNote(`Added object ${row.id}. Use this ID for dynamic objects in npcgen.data.`);
  };
  const add = useCallback(() => { if (file) setPicking({ key: null, kind: "model" }); }, [file]);
  const remove = useCallback((row: Row) => {
    const uses = check?.uses[String(row.id)] ?? 0;
    if (!window.confirm(`Delete object ${row.id} (${row.path || "no path"})?${uses ? `\n\nThe open npcgen.data places ${count(uses)} object${uses === 1 ? "" : "s"} with this ID: they would show nothing.` : ""}\n\nUndo brings it back.`)) return;
    commit(rows.filter((candidate) => candidate.key !== row.key), `delete:${row.key}`);
    setSelected(null);
  }, [check, commit, rows]);
  const selectedRow = rows.find((row) => row.key === selected) ?? null;
  const deleteSelected = useCallback(() => { if (selectedRow) remove(selectedRow); }, [remove, selectedRow]);
  const undo = useCallback(() => setHistory((current) => current.cursor > 0 ? { ...current, cursor: current.cursor - 1 } : current), []);
  const redo = useCallback(() => setHistory((current) => current.cursor + 1 < current.states.length ? { ...current, cursor: current.cursor + 1 } : current), []);

  const saveTo = useCallback(async (targetPath: string, replaceChanged = false) => {
    if (!file || busy) return;
    const errors = (check?.problems ?? []).filter((problem) => problem.severity === "error");
    if (!replaceChanged && errors.length) {
      setProblemsOpen(true);
      setError(`Fix ${count(errors.length)} error${errors.length === 1 ? "" : "s"} first: the client would refuse the whole table.`);
      return;
    }
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const report = await saveDynObjects({ openedPath: file.path, targetPath, token: file.token, rows: rows.map(({ id, path }) => ({ id, path })), backup: backup && !backedUp.current.has(pathKey(targetPath)), replaceChanged });
      const sorted = [...rows].sort((a, b) => a.id - b.id);
      savedRows.current = sorted;
      savedByKey.current = new Map(sorted.map((row) => [row.key, row]));
      if (report.backup) backedUp.current.add(pathKey(report.path));
      setHistory({ states: [sorted], cursor: 0, keys: [] });
      setFile({ path: report.path, size: report.size, token: report.token, rows: sorted.map(({ id, path }) => ({ id, path })) });
      setNote(`Saved ${count(report.rows)} objects.${report.backup ? ` Backup: ${report.backup}.` : ""} Only the client reads this file: every player needs the new copy (element\\data\\DynamicObjects.data).`);
    } catch (problem) {
      const message = messageOf(problem);
      if (message.startsWith("CHANGED_ON_DISK:") && window.confirm(`${message.slice("CHANGED_ON_DISK:".length).trim()}\n\nReplace it with the open table?`)) {
        setBusy(false);
        await saveTo(targetPath, true);
        return;
      }
      setError(message);
    } finally {
      setBusy(false);
    }
  }, [backup, busy, check, file, rows]);
  const saveCurrent = useCallback(() => { if (file && dirty) void saveTo(file.path); }, [dirty, file, saveTo]);
  const saveAs = useCallback(async () => {
    if (!file || busy) return;
    const target = await save({ defaultPath: file.path, title: "Save DynamicObjects.data as", filters: [{ name: "DynamicObjects.data", extensions: ["data"] }] });
    if (typeof target === "string") await saveTo(target);
  }, [busy, file, saveTo]);

  const exportJson = useCallback(async () => {
    if (!file || busy) return;
    const target = await save({ defaultPath: file.path.replace(/\.data$/i, ".json"), title: "Export dynamic objects as JSON", filters: [{ name: "JSON", extensions: ["json"] }] });
    if (typeof target !== "string") return;
    try {
      const report = await exportDynObjectsJson(target, file.path, rows.map(({ id, path }) => ({ id, path })));
      setNote(`Exported ${count(report.rows)} objects to ${fileName(report.path)}.`);
    } catch (problem) {
      setError(messageOf(problem));
    }
  }, [busy, file, rows]);
  const importJson = useCallback(async () => {
    if (!file || busy) return;
    const source = await open({ multiple: false, directory: false, defaultPath: file.path, title: "Import dynamic objects from JSON", filters: [{ name: "JSON", extensions: ["json"] }] });
    if (typeof source !== "string") return;
    try {
      const imported = await importDynObjectsJson(source);
      const current = new Map(rows.map((row) => [row.id, row.path]));
      const incoming = new Set(imported.rows.map((row) => row.id));
      const added = imported.rows.filter((row) => !current.has(row.id)).length;
      const changed = imported.rows.filter((row) => current.has(row.id) && current.get(row.id) !== row.path).length;
      const removed = rows.filter((row) => !incoming.has(row.id)).length;
      if (!added && !changed && !removed) { setNote(`${fileName(source)} matches the open table.`); return; }
      if (!window.confirm(`Replace the open table with the ${count(imported.rows.length)} JSON rows?\n\n${count(added)} added · ${count(changed)} changed · ${count(removed)} removed\n\nUndo brings the old table back.`)) return;
      const next = imported.rows.map((row) => ({ ...row, key: nextKey.current++ })).sort((a, b) => a.id - b.id);
      commit(next, `json:${source}:${Date.now()}`);
      setSelected(next[0]?.key ?? null);
      setNote(`Imported ${count(next.length)} objects from ${fileName(source)}. Save to write DynamicObjects.data.`);
    } catch (problem) {
      setError(messageOf(problem));
    }
  }, [busy, commit, file, rows]);

  const compare = useCallback(async () => {
    if (!file) return;
    const picked = await open({ multiple: false, directory: false, defaultPath: file.path, title: "Compare with another DynamicObjects.data or a JSON export", filters: [{ name: "DynamicObjects.data or JSON", extensions: ["data", "json"] }] });
    if (typeof picked !== "string") return;
    try {
      const table = picked.toLowerCase().endsWith(".json") ? (await importDynObjectsJson(picked)).rows : (await openDynObjects(picked)).rows;
      setOther({ path: picked, rows: table });
    } catch (problem) {
      setError(messageOf(problem));
    }
  }, [file]);
  const copyFrom = (copy: PathDataRow[]) => {
    const byId = new Map(copy.map((row) => [row.id, row.path]));
    const next = rows.map((row) => byId.has(row.id) ? { ...row, path: byId.get(row.id)! } : row);
    const known = new Set(rows.map((row) => row.id));
    for (const row of copy) if (!known.has(row.id)) next.push({ ...row, key: nextKey.current++ });
    next.sort((a, b) => a.id - b.id);
    commit(next, `copy:${Date.now()}`);
    setNote(`Copied ${count(copy.length)} object${copy.length === 1 ? "" : "s"} from ${fileName(other?.path ?? "")}. Undo takes them back.`);
  };

  useImperativeHandle(ref, () => ({ choose: () => void choose(), openPath: (path) => guard(() => void load(path)), save: saveCurrent, saveAs: () => void saveAs(), exportJson: () => void exportJson(), importJson: () => void importJson(), undo, redo, add, deleteSelected, openProblems: () => setProblemsOpen(true), compare: () => void compare() }), [add, compare, choose, deleteSelected, exportJson, guard, importJson, load, redo, saveAs, saveCurrent, undo]);
  useEffect(() => onStateChange({ loaded: !!file, dirty, canUndo: history.cursor > 0, canRedo: history.cursor + 1 < history.states.length, path: file?.path ?? null, objects: rows.length }), [dirty, file, history.cursor, history.states.length, onStateChange, rows.length]);

  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      if (picking || other) return;
      const mod = event.ctrlKey || event.metaKey;
      const typing = event.target instanceof HTMLElement && /^(INPUT|TEXTAREA|SELECT)$/.test(event.target.tagName);
      const key = event.key.toLowerCase();
      if (event.key === "Escape" && problemsOpen) { event.preventDefault(); setProblemsOpen(false); return; }
      if (mod && event.shiftKey && key === "m") { event.preventDefault(); event.stopImmediatePropagation(); setProblemsOpen(true); return; }
      if (mod && ["o", "s", "z", "y"].includes(key)) {
        if (typing && (key === "z" || key === "y")) return;
        event.preventDefault();
        event.stopImmediatePropagation();
        if (key === "o") void choose();
        else if (key === "s") event.shiftKey ? void saveAs() : saveCurrent();
        else if (key === "z") event.shiftKey ? redo() : undo();
        else redo();
      } else if (event.key === "Delete" && !typing) {
        event.preventDefault();
        deleteSelected();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [active, choose, deleteSelected, other, picking, problemsOpen, redo, saveAs, saveCurrent, undo]);

  const idCounts = useMemo(() => {
    const counts = new Map<number, number>();
    for (const row of rows) counts.set(row.id, (counts.get(row.id) ?? 0) + 1);
    return counts;
  }, [rows]);
  const kinds = useMemo(() => {
    const out = { model: 0, effect: 0, other: 0 };
    for (const row of rows) { const kind = kindOf(row.path); if (kind === "model" || kind === "effect") out[kind]++; else out.other++; }
    return out;
  }, [rows]);
  const visible = useMemo(() => rows
    .filter((row) => !deferredQuery || String(row.id) === deferredQuery || row.path.toLowerCase().includes(deferredQuery))
    .filter((row) => filter === "all" || (filter === "problems" ? problemIds.has(row.id) : filter === "unused" ? !usesOf(row.id) : kindOf(row.path) === filter))
    .sort((a, b) => a.id - b.id),
  // usesOf reads check, listed below.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  [check, deferredQuery, filter, problemIds, rows]);
  const pages = Math.max(1, Math.ceil(visible.length / PAGE_SIZE));
  useEffect(() => setPage((current) => Math.min(current, pages - 1)), [pages]);
  const shown = visible.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);
  const selectedInfo = selectedRow ? infoByKey.get(selectedRow.key) : undefined;
  const errors = (check?.problems ?? []).filter((problem) => problem.severity === "error").length;
  const warnings = (check?.problems ?? []).length - errors;

  if (!file) return <section className="path-data-pane empty">
    <div className="drop-card path-data-empty">
      <Box size={30} />
      <h2>Open DynamicObjects.data</h2>
      <p className="muted">The client's models and effects for npcgen.data's dynamic objects (doors, portals, scene effects): an ID per object and the file the client shows. Only the client reads it.</p>
      <button className="btn primary" onClick={() => void choose()} disabled={busy}><FolderOpen size={14} /> Choose DynamicObjects.data…</button>
      {defaultPath && <button className="btn" onClick={() => void load(defaultPath)} disabled={busy}>Open the configured client's file</button>}
      {error && <div className="se-problems" role="alert">{error}</div>}
    </div>
  </section>;

  return <section className="path-data-pane">
    <header className="path-data-head">
      <div>
        <h2>Dynamic objects {dirty && <span className="tag warn">unsaved</span>}</h2>
        <div className="path-data-file-line">
          <span className="path-data-location mono muted small truncate" title={file.path}>{file.path}</span>
          <span className="path-data-badge"><b>Objects:</b> {count(rows.length)}</span>
          <span className="path-data-badge"><b>Models:</b> {count(kinds.model)}</span>
          <span className="path-data-badge"><b>Effects:</b> {count(kinds.effect)}</span>
          <span className="path-data-badge"><b>Size:</b> {bytes(file.size)}</span>
        </div>
      </div>
      <button className="btn" onClick={add} disabled={busy} title="A new object: choose its model or effect; it gets the next free ID"><Plus size={13} /> Add object…</button>
      <button className="icon-btn" onClick={undo} disabled={busy || history.cursor <= 0} title="Undo (Ctrl+Z)"><Undo2 size={15} /></button>
      <button className="icon-btn" onClick={redo} disabled={busy || history.cursor + 1 >= history.states.length} title="Redo (Ctrl+Y)"><Redo2 size={15} /></button>
      <button className="btn" onClick={() => setProblemsOpen(true)} title="Problems (Ctrl+Shift+M)">{checking ? <Loader2 size={13} className="spin" /> : errors ? <CircleAlert size={13} className="danger-icon" /> : <TriangleAlert size={13} />} Problems{check ? ` ${count(check.problems.length)}` : ""}</button>
      <button className="btn primary" onClick={saveCurrent} disabled={busy || !dirty}><Save size={13} /> Save</button>
      <button className="btn" onClick={() => void choose()} disabled={busy}><FolderOpen size={13} /> Open…</button>
    </header>
    <div className="path-data-tools">
      <label className="path-data-search"><Search size={14} /><input value={query} onChange={(event) => { setQuery(event.target.value); setPage(0); }} placeholder="ID or words of the path…" /></label>
      <span className="dyn-compare-tabs">
        {([["all", "All"], ["model", "Models"], ["effect", "Effects"], ["problems", "With problems"], ["unused", "Not in npcgen"]] as [Filter, string][]).map(([value, label]) => <button key={value} className={"btn small" + (filter === value ? " active" : "")} disabled={value === "unused" && !check?.npcgen} title={value === "unused" ? check?.npcgen ? `Objects the open npcgen.data (${fileName(check.npcgen)}) does not place` : "Open an npcgen.data in its editor" : undefined} onClick={() => { setFilter(value); setPage(0); }}>{label}</button>)}
      </span>
      <span className="muted small">{count(visible.length)} shown</span>
      <span className="spacer" />
      <label className="check"><input type="checkbox" checked={backup} onChange={(event) => { setBackup(event.target.checked); try { localStorage.setItem(BACKUP_KEY, event.target.checked ? "1" : "0"); } catch { /* optional */ } }} /> Backup replaced file</label>
      <button className="btn small" onClick={() => void saveAs()} disabled={busy}>Save as…</button>
    </div>
    {(error || note) && <div className={error ? "path-data-message error" : "path-data-message"} role={error ? "alert" : "status"}>
      {error ? <AlertTriangle size={14} /> : <Info size={14} />}{error || note} <button className="link" onClick={() => { setError(null); setNote(null); }}>Dismiss</button>
    </div>}
    <div className="path-data-body">
      <div className="path-data-table-wrap">
        <table className="path-data-table dyn-objects-table">
          <thead><tr><th>ID</th><th>File</th><th>Kind</th><th title="Whether the configured client has the file">Client</th><th title="Objects the open npcgen.data places with this ID">npcgen</th><th /></tr></thead>
          <tbody>{shown.map((row) => {
            const info = infoByKey.get(row.key);
            const saved = savedByKey.current.get(row.key);
            const kind = kindOf(row.path);
            const badId = row.id === 0 || (idCounts.get(row.id) ?? 0) > 1;
            return <tr key={row.key} className={(row.key === selected ? "selected " : "") + (!saved || saved.id !== row.id || saved.path !== row.path ? "changed" : "")} onClick={() => setSelected(row.key)}>
              <td><input className={badId ? "invalid mono" : "mono"} type="number" min={1} value={row.id || ""} onChange={(event) => change(row.key, "id", event.target.value)} aria-label={`Object ID ${row.id}`} /></td>
              <td><span className="dyn-object-path"><input className={(kind === "empty" || kind === "other" || info?.exists === false ? "invalid " : "") + "mono"} value={row.path} onChange={(event) => change(row.key, "path", event.target.value)} spellCheck={false} placeholder="(no file: shows nothing)" aria-label={`File of object ${row.id}`} /><button className="icon-btn small" title="Choose from the client's models and effects" onClick={(event) => { event.stopPropagation(); setSelected(row.key); setPicking({ key: row.key, kind: kind === "effect" ? "effect" : "model", current: row.path }); }}><Search size={12} /></button></span></td>
              <td>{kind === "model" ? <span className="dyn-kind"><Box size={12} /> model</span> : kind === "effect" ? <span className="dyn-kind"><Sparkles size={12} /> effect</span> : <span className="ai-warn small">{KIND_LABELS[kind]}</span>}</td>
              <td>{info?.exists === true ? <span className="dyn-ok">✓</span> : info?.exists === false ? <span className="ai-warn" title={`${info.file} is not in the client`}>missing</span> : <span className="muted">—</span>}</td>
              <td className="mono">{check?.npcgen ? usesOf(row.id) || <span className="muted">0</span> : <span className="muted">—</span>}</td>
              <td><button className="icon-btn small danger" onClick={(event) => { event.stopPropagation(); remove(row); }} title={`Delete object ${row.id} (Del)`}><Trash2 size={13} /></button></td>
            </tr>;
          })}</tbody>
        </table>
        {!shown.length && <div className="empty-note center">No object matches.</div>}
      </div>
      <aside className="path-data-info">
        <h3>{selectedRow ? `Object ${selectedRow.id}` : "Object"}</h3>
        {selectedRow ? <>
          <dl>
            <dt>Stored path</dt><dd className="mono">{selectedRow.path || "(empty)"}</dd>
            <dt>Client loads</dt><dd className="mono">{selectedInfo?.kind === "model" || selectedInfo?.kind === "effect" ? selectedInfo.file : "nothing"}</dd>
            <dt>In the client</dt><dd>{selectedInfo?.exists === true ? "yes" : selectedInfo?.exists === false ? <span className="ai-warn">no: the object shows nothing</span> : check?.client ? "—" : "no client configured"}</dd>
            <dt>npcgen.data</dt><dd>{check?.npcgen ? `${count(usesOf(selectedRow.id))} object${usesOf(selectedRow.id) === 1 ? "" : "s"} in ${fileName(check.npcgen)}` : "open a map in the NPC Gen editor to count its objects"}</dd>
          </dl>
          {(check?.problems ?? []).filter((problem) => problem.id === selectedRow.id).map((problem, index) => <div key={index} className={"path-data-message" + (problem.severity === "error" ? " error" : "")}><TriangleAlert size={13} /> {problem.message}</div>)}
        </> : <p className="muted small">Select an object.</p>}
        <div className="path-data-warning"><Info size={14} /><span>npcgen.data's dynamic objects use these IDs; the client shows the model (.ecm) or effect (.gfx, below gfx\). Only the client reads this file, so players need the edited copy. A repeated ID or path makes the client drop the whole table.</span></div>
      </aside>
    </div>
    <footer className="path-data-foot">
      <span>Rows {visible.length ? page * PAGE_SIZE + 1 : 0}–{Math.min((page + 1) * PAGE_SIZE, visible.length)} of {count(visible.length)}</span>
      <span className="spacer" />
      <button className="btn small" onClick={() => setPage((value) => Math.max(0, value - 1))} disabled={page === 0}>Previous</button>
      <span className="muted small">Page {page + 1} of {pages}</span>
      <button className="btn small" onClick={() => setPage((value) => Math.min(pages - 1, value + 1))} disabled={page + 1 >= pages}>Next</button>
    </footer>
    {other && <DynObjectsCompare other={other} rows={rows.map(({ id, path }) => ({ id, path }))} onClose={() => setOther(null)} onChooseOther={() => void compare()} onCopy={copyFrom} />}
    {picking && <DynFilePicker kind={picking.kind} current={picking.current} onClose={() => setPicking(null)} onPick={(path) => { if (picking.key === null) addWith(path); else change(picking.key, "path", path); setPicking(null); }} />}
    {problemsOpen && <div className="modal-backdrop" onMouseDown={() => setProblemsOpen(false)}>
      <div className="modal npcgen-problems-dialog" role="dialog" aria-label="Problems" onMouseDown={(event) => event.stopPropagation()}>
        <header className="modal-head"><h3>Problems</h3>{check && <span className="muted small">{count(errors)} error{errors === 1 ? "" : "s"} · {count(warnings)} warning{warnings === 1 ? "" : "s"}</span>}<span className="spacer" /><button className="icon-btn" onClick={() => setProblemsOpen(false)} aria-label="Close"><X size={16} /></button></header>
        <div className="npcgen-problems-list">
          {!check ? <div className="empty-note"><Loader2 size={14} className="spin" /> Checking…</div> : !check.problems.length ? <div className="empty-note">No problems.</div> : check.problems.map((problem, index) => <button key={index} className={"npcgen-problem " + problem.severity} onClick={() => { const row = rows.find((entry) => entry.id === problem.id); if (row) { setSelected(row.key); setQuery(""); setFilter("all"); setPage(Math.floor(rows.slice().sort((a, b) => a.id - b.id).findIndex((entry) => entry.key === row.key) / PAGE_SIZE)); } setProblemsOpen(false); }}>
            {problem.severity === "error" ? <CircleAlert size={14} /> : <TriangleAlert size={14} />}
            <span>{problem.id !== null && <b>Object {problem.id}</b>} {problem.message}</span>
          </button>)}
        </div>
        <footer className="modal-foot muted small">Rules from the client (DataPathMan, CECMatter): errors make it drop the whole table; warnings leave an object without a model.{check && !check.client ? " Set the game client in Settings to check the files." : ""}{check && !check.npcgen ? " Open a map in the NPC Gen editor to check its object IDs." : ""}</footer>
      </div>
    </div>}
  </section>;
});
