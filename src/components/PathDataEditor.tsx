import { forwardRef, useCallback, useDeferredValue, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, FilePlus2, FolderOpen, Redo2, RotateCcw, Save, Search, Trash2, TriangleAlert, Undo2, X } from "lucide-react";
import { exportPathDataJson, importPathDataJson, openPathData, savePathData } from "../elements/api";
import { bytes, count } from "../elements/format";
import type { PathDataFile, PathDataRow } from "../elements/types";

interface EditorRow extends PathDataRow {
  key: number;
}

interface History {
  states: EditorRow[][];
  cursor: number;
  /** Key of the edit that produced states[n + 1], for typing coalescing. */
  keys: string[];
}

export interface PathDataEditorState {
  loaded: boolean;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  path: string | null;
}

export interface PathDataEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
  save: () => void;
  saveAs: () => void;
  exportJson: () => void;
  importJson: () => void;
  undo: () => void;
  redo: () => void;
}

interface Props {
  defaultPath?: string | null;
  active: boolean;
  onStateChange: (state: PathDataEditorState) => void;
  onClientReloaded?: () => void;
}

const PAGE_SIZE = 200;
const BACKUP_KEY = "jdide.pathData.backup";

const fileName = (path: string) => path.split(/[\\/]/).pop() || path;
const pathKey = (path: string) => path.replaceAll("/", "\\").toLowerCase();
const extension = (path: string) => {
  const name = path.split(/[\\/]/).pop() || path;
  const at = name.lastIndexOf(".");
  return at > 0 ? name.slice(at + 1).toLowerCase() : "—";
};
export const PathDataEditor = forwardRef<PathDataEditorHandle, Props>(function PathDataEditor({ defaultPath, active, onStateChange, onClientReloaded }, ref) {
  const [file, setFile] = useState<PathDataFile | null>(null);
  const [history, setHistory] = useState<History>({ states: [], cursor: -1, keys: [] });
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query.trim().toLowerCase());
  const [page, setPage] = useState(0);
  const [pageInput, setPageInput] = useState("1");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [backup, setBackup] = useState(() => localStorage.getItem(BACKUP_KEY) !== "0");
  const [selected, setSelected] = useState<number | null>(null);
  const [pendingDelete, setPendingDelete] = useState<EditorRow | null>(null);
  const savedRows = useRef<EditorRow[] | null>(null);
  const savedByKey = useRef(new Map<number, EditorRow>());
  const nextKey = useRef(1);
  const autoOpened = useRef<string | null>(null);
  const backedUp = useRef(new Set<string>());
  const rows = history.cursor >= 0 ? history.states[history.cursor] : [];
  const dirty = !!file && rows !== savedRows.current;

  const setBackupRemembered = (value: boolean) => {
    setBackup(value);
    localStorage.setItem(BACKUP_KEY, value ? "1" : "0");
  };

  const load = useCallback(async (path: string) => {
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const opened = await openPathData(path);
      const loaded = opened.rows.map((row) => ({ ...row, key: nextKey.current++ }));
      savedRows.current = loaded;
      savedByKey.current = new Map(loaded.map((row) => [row.key, row]));
      setFile(opened);
      setHistory({ states: [loaded], cursor: 0, keys: [] });
      setQuery("");
      setPage(0);
      setSelected(loaded[0]?.key ?? null);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
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
    if (!dirty || window.confirm("Discard the unsaved path.data changes?")) action();
  }, [dirty]);

  const choose = useCallback(async () => {
    const picked = await open({
      multiple: false,
      directory: false,
      defaultPath: file?.path || defaultPath || undefined,
      title: "Open path.data",
      filters: [{ name: "path.data", extensions: ["data"] }],
    });
    if (typeof picked === "string") guard(() => void load(picked));
  }, [defaultPath, file?.path, guard, load]);

  const commit = useCallback((next: EditorRow[], key: string) => {
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
      if (states.length > 101) {
        states.shift();
        keys.shift();
        return { states, cursor: states.length - 1, keys };
      }
      return { states, cursor: current.cursor + 1, keys };
    });
    setNote(null);
  }, []);

  const change = (key: number, field: "id" | "path", value: string) => {
    const next = rows.map((row) => row.key === key ? { ...row, [field]: field === "id" ? Math.max(0, Math.min(0xffff_ffff, Number(value) || 0)) : value } : row);
    commit(next, `field:${key}:${field}`);
  };

  const add = () => {
    const highest = rows.reduce((max, row) => Math.max(max, row.id), 0);
    if (highest >= 0xffff_ffff) {
      setError("No higher path ID is available.");
      return;
    }
    const row: EditorRow = { key: nextKey.current++, id: highest + 1, path: "" };
    commit([...rows, row], `add:${row.key}`);
    setQuery("");
    setPage(Math.floor(rows.length / PAGE_SIZE));
    setSelected(row.key);
    setTimeout(() => document.querySelector<HTMLInputElement>(`[data-path-key="${row.key}"]`)?.focus());
  };

  const remove = (row: EditorRow) => {
    commit(rows.filter((candidate) => candidate.key !== row.key), `delete:${row.key}`);
    if (selected === row.key) setSelected(null);
    setPendingDelete(null);
  };

  useEffect(() => {
    if (!pendingDelete) return;
    const onKey = (event: KeyboardEvent) => event.key === "Escape" && setPendingDelete(null);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pendingDelete]);

  const undo = useCallback(() => setHistory((current) => current.cursor > 0 ? { ...current, cursor: current.cursor - 1 } : current), []);
  const redo = useCallback(() => setHistory((current) => current.cursor + 1 < current.states.length ? { ...current, cursor: current.cursor + 1 } : current), []);

  const saveTo = useCallback(async (targetPath: string, replaceChanged = false) => {
    if (!file || busy) return;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const report = await savePathData({
        openedPath: file.path,
        targetPath,
        token: file.token,
        rows: rows.map(({ id, path }) => ({ id, path })),
        backup: backup && !backedUp.current.has(pathKey(targetPath)),
        replaceChanged,
      });
      const sorted = [...rows].sort((a, b) => a.id - b.id);
      savedRows.current = sorted;
      savedByKey.current = new Map(sorted.map((row) => [row.key, row]));
      if (report.backup) backedUp.current.add(pathKey(report.path));
      setHistory({ states: [sorted], cursor: 0, keys: [] });
      setFile({ path: report.path, size: report.size, token: report.token, rows: sorted.map(({ id, path }) => ({ id, path })) });
      if (report.clientReloaded) onClientReloaded?.();
      setNote(report.siblingElements
        ? `Saved ${count(report.rows)} paths. Save ${fileName(report.siblingElements)} again so its checksum uses this path.data.`
        : `Saved ${count(report.rows)} paths.`);
    } catch (problem) {
      const message = String(problem).replace(/^Error: /, "");
      if (message.startsWith("CHANGED_ON_DISK:") && window.confirm(`${message.slice("CHANGED_ON_DISK:".length).trim()}\n\nReplace it with the open path table?`)) {
        setBusy(false);
        await saveTo(targetPath, true);
        return;
      }
      setError(message);
    } finally {
      setBusy(false);
    }
  }, [backup, busy, file, onClientReloaded, rows]);

  const saveCurrent = useCallback(() => {
    if (file) void saveTo(file.path);
  }, [file, saveTo]);

  const saveAs = useCallback(async () => {
    if (!file || busy) return;
    const target = await save({
      defaultPath: file.path,
      title: "Save path.data as",
      filters: [{ name: "path.data", extensions: ["data"] }],
    });
    if (typeof target === "string") await saveTo(target);
  }, [busy, file, saveTo]);

  const exportJson = useCallback(async () => {
    if (!file || busy) return;
    const target = await save({
      defaultPath: file.path.replace(/\.data$/i, "-paths.json"),
      title: "Export path table as JSON",
      filters: [{ name: "JD IDE path table", extensions: ["json"] }],
    });
    if (typeof target !== "string") return;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const report = await exportPathDataJson(target, file.path, rows.map(({ id, path }) => ({ id, path })));
      setNote(`Exported ${count(report.rows)} paths to ${fileName(report.path)}.`);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  }, [busy, file, rows]);

  const importJson = useCallback(async () => {
    if (!file || busy) return;
    const source = await open({
      multiple: false,
      directory: false,
      defaultPath: file.path,
      title: "Import path table from JSON",
      filters: [{ name: "JD IDE path table", extensions: ["json"] }],
    });
    if (typeof source !== "string") return;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const imported = await importPathDataJson(source);
      const current = new Map(rows.map((row) => [row.id, row.path]));
      const incoming = new Map(imported.rows.map((row) => [row.id, row.path]));
      const added = imported.rows.filter((row) => !current.has(row.id)).length;
      const changed = imported.rows.filter((row) => current.has(row.id) && current.get(row.id) !== row.path).length;
      const removed = rows.filter((row) => !incoming.has(row.id)).length;
      if (!added && !changed && !removed) {
        setNote(`${fileName(source)} matches the open path table.`);
        return;
      }
      const origin = imported.sourcePath ? `\nExported from: ${imported.sourcePath}` : "";
      if (!window.confirm(`Replace the open path table with ${count(imported.rows.length)} JSON rows?\n\n${count(added)} added · ${count(changed)} changed · ${count(removed)} removed${origin}\n\nThe replacement is undoable until the binary file is saved.`)) return;
      const next = imported.rows.map((row) => ({ ...row, key: nextKey.current++ })).sort((a, b) => a.id - b.id);
      commit(next, `json:${source}:${Date.now()}`);
      setQuery("");
      setPage(0);
      setSelected(next[0]?.key ?? null);
      setNote(`Imported ${count(next.length)} paths from ${fileName(source)}. Save to write path.data.`);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  }, [busy, commit, file, rows]);

  useImperativeHandle(ref, () => ({ choose: () => void choose(), openPath: (path) => guard(() => void load(path)), save: saveCurrent, saveAs: () => void saveAs(), exportJson: () => void exportJson(), importJson: () => void importJson(), undo, redo }), [choose, exportJson, guard, importJson, load, redo, saveAs, saveCurrent, undo]);

  useEffect(() => onStateChange({
    loaded: !!file,
    dirty,
    canUndo: history.cursor > 0,
    canRedo: history.cursor + 1 < history.states.length,
    path: file?.path ?? null,
  }), [dirty, file, history.cursor, history.states.length, onStateChange]);

  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      const mod = event.ctrlKey || event.metaKey;
      if (!mod) return;
      const key = event.key.toLowerCase();
      if (!["o", "s", "z", "y"].includes(key)) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      if (key === "o") void choose();
      else if (key === "s") event.shiftKey ? void saveAs() : saveCurrent();
      else if (key === "z") event.shiftKey ? redo() : undo();
      else redo();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [active, choose, redo, saveAs, saveCurrent, undo]);

  const idCounts = useMemo(() => {
    const counts = new Map<number, number>();
    for (const row of rows) counts.set(row.id, (counts.get(row.id) ?? 0) + 1);
    return counts;
  }, [rows]);
  const pathCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const row of rows) counts.set(row.path, (counts.get(row.path) ?? 0) + 1);
    return counts;
  }, [rows]);
  const visible = useMemo(() => rows
    .map((row) => row)
    .filter((row) => !deferredQuery || String(row.id).includes(deferredQuery) || row.path.toLowerCase().includes(deferredQuery))
    .sort((a, b) => a.id - b.id), [deferredQuery, rows]);
  const pages = Math.max(1, Math.ceil(visible.length / PAGE_SIZE));
  useEffect(() => setPage((current) => Math.min(current, pages - 1)), [pages]);
  useEffect(() => setPageInput(String(page + 1)), [page]);
  const applyPageInput = () => {
    const requested = Number.parseInt(pageInput, 10);
    const next = Number.isFinite(requested) ? Math.max(1, Math.min(pages, requested)) : page + 1;
    setPage(next - 1);
    setPageInput(String(next));
  };
  const shown = visible.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);
  const selectedRow = rows.find((row) => row.key === selected) ?? null;

  if (!file) {
    return (
      <section className="path-data-pane empty">
        <div className="drop-card path-data-empty">
          <FilePlus2 size={30} />
          <h2>Open path.data</h2>
          <p className="muted">Browse and edit the client resource IDs independently from elements.data.</p>
          <button className="btn primary" onClick={() => void choose()} disabled={busy}><FolderOpen size={14} /> Choose path.data…</button>
          {defaultPath && <button className="btn" onClick={() => void load(defaultPath)} disabled={busy}>Open configured client file</button>}
          {error && <div className="se-problems" role="alert">{error}</div>}
        </div>
      </section>
    );
  }

  return (
    <section className="path-data-pane">
      <header className="path-data-head">
        <div>
          <h2>Path data editor</h2>
          <div className="path-data-file-line">
            <span className="path-data-location mono muted small truncate" title={file.path}>{file.path}</span>
            <span className="path-data-badge"><b>Paths:</b> {count(rows.length)}</span>
            <span className="path-data-badge"><b>Size:</b> {bytes(file.size)}</span>
          </div>
        </div>
        {dirty && <span className="dirty-label">Unsaved changes</span>}
        <button className="btn" onClick={() => void choose()} disabled={busy}><FolderOpen size={13} /> Open…</button>
        <button className="btn" onClick={add} disabled={busy}><FilePlus2 size={13} /> Add path</button>
        <button className="icon-btn" onClick={undo} disabled={busy || history.cursor <= 0} title="Undo (Ctrl+Z)"><Undo2 size={15} /></button>
        <button className="icon-btn" onClick={redo} disabled={busy || history.cursor + 1 >= history.states.length} title="Redo (Ctrl+Y)"><Redo2 size={15} /></button>
        <button className="btn primary" onClick={saveCurrent} disabled={busy || !dirty}><Save size={13} /> Save</button>
      </header>
      <div className="path-data-tools">
        <label className="path-data-search"><Search size={14} /><input value={query} onChange={(event) => { setQuery(event.target.value); setPage(0); }} placeholder="Search by ID or path…" /></label>
        <span>{count(visible.length)} match{visible.length === 1 ? "" : "es"}</span>
        <span className="spacer" />
        <label className="check"><input type="checkbox" checked={backup} onChange={(event) => setBackupRemembered(event.target.checked)} /> Backup replaced file</label>
        <button className="btn small" onClick={() => void saveAs()} disabled={busy}>Save as…</button>
      </div>
      {(error || note) && <div className={error ? "path-data-message error" : "path-data-message"} role={error ? "alert" : "status"}>
        {error ? <AlertTriangle size={14} /> : <RotateCcw size={14} />}{error || note}
      </div>}
      <div className="path-data-body">
        <div className="path-data-table-wrap">
          <table className="path-data-table">
            <thead><tr><th>ID</th><th>Path</th><th>Type</th><th /></tr></thead>
            <tbody>{shown.map((row) => {
              const badId = row.id === 0 || (idCounts.get(row.id) ?? 0) > 1;
              const badPath = !row.path || (pathCounts.get(row.path) ?? 0) > 1;
              const changed = savedByKey.current.get(row.key);
              return <tr key={row.key} className={(row.key === selected ? "selected " : "") + (!changed || changed.id !== row.id || changed.path !== row.path ? "changed" : "")} onClick={() => setSelected(row.key)}>
                <td><input className={badId ? "invalid mono" : "mono"} type="number" min={1} max={0xffff_ffff} value={row.id || ""} onChange={(event) => change(row.key, "id", event.target.value)} aria-label={`Path ID ${row.id}`} /></td>
                <td><input className={badPath ? "invalid mono" : "mono"} data-path-key={row.key} value={row.path} onChange={(event) => change(row.key, "path", event.target.value)} spellCheck={false} aria-label={`Path for ID ${row.id}`} /></td>
                <td className="mono muted">{extension(row.path)}</td>
                <td><button className="icon-btn small danger" onClick={(event) => { event.stopPropagation(); setPendingDelete(row); }} title={`Delete ID ${row.id}`}><Trash2 size={13} /></button></td>
              </tr>;
            })}</tbody>
          </table>
          {!shown.length && <div className="empty-note center">No paths match this search.</div>}
        </div>
        <aside className="path-data-info">
          <h3>{selectedRow ? `Path ID ${selectedRow.id}` : "Path details"}</h3>
          {selectedRow ? <>
            <dl><dt>Path</dt><dd className="mono">{selectedRow.path || "(empty)"}</dd><dt>File type</dt><dd>{extension(selectedRow.path)}</dd></dl>
            <p className="muted small">The path is stored as GBK bytes without a terminator. The client safely supports up to 255 bytes.</p>
          </> : <p className="muted small">Select a row to inspect it.</p>}
          <div className="path-data-warning"><AlertTriangle size={14} /><span>After changing this file, save the matching elements.data again so its client checksum includes the new path.data.</span></div>
        </aside>
      </div>
      <footer className="path-data-foot">
        <span>Rows {visible.length ? page * PAGE_SIZE + 1 : 0}–{Math.min((page + 1) * PAGE_SIZE, visible.length)} of {count(visible.length)}</span>
        <span className="spacer" />
        <button className="btn small" onClick={() => setPage(0)} disabled={page === 0}>First</button>
        <button className="btn small" onClick={() => setPage((value) => Math.max(0, value - 1))} disabled={page === 0}>Previous</button>
        <label className="path-page-jump">Page <input type="number" min={1} max={pages} value={pageInput} onChange={(event) => setPageInput(event.target.value)} onBlur={applyPageInput} onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); applyPageInput(); event.currentTarget.select(); } }} aria-label={`Page number, 1 to ${pages}`} /> of {pages}</label>
        <button className="btn small" onClick={() => setPage((value) => Math.min(pages - 1, value + 1))} disabled={page + 1 >= pages}>Next</button>
        <button className="btn small" onClick={() => setPage(pages - 1)} disabled={page + 1 >= pages}>Last</button>
      </footer>
      {pendingDelete && (
        <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && setPendingDelete(null)}>
          <div className="modal delete-dialog" role="alertdialog" aria-modal="true" aria-label={`Delete path ID ${pendingDelete.id}`}>
            <header className="modal-head">
              <Trash2 size={16} className="danger-icon" />
              <h3 className="truncate">Delete path ID {pendingDelete.id}?</h3>
              <span className="spacer" />
              <button className="icon-btn" onClick={() => setPendingDelete(null)} aria-label="Close"><X size={18} /></button>
            </header>
            <div className="delete-body">
              <p>This entry will be removed from the path table. Undo (Ctrl+Z) brings it back until the binary file is saved.</p>
              <div className="delete-warning">
                <div className="delete-warning-head"><TriangleAlert size={15} /> elements.data fields using ID {pendingDelete.id} will no longer resolve this resource.</div>
                <div className="mono small path-delete-value">{pendingDelete.path}</div>
              </div>
            </div>
            <footer className="modal-foot">
              <span className="spacer" />
              <button className="btn" onClick={() => setPendingDelete(null)} autoFocus>Cancel</button>
              <button className="btn danger-solid" onClick={() => remove(pendingDelete)}><Trash2 size={14} /> Delete</button>
            </footer>
          </div>
        </div>
      )}
    </section>
  );
});
