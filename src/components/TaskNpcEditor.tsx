import { forwardRef, useCallback, useDeferredValue, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, Eraser, FolderOpen, MapPin, Plus, Redo2, RotateCcw, Save, Search, Trash2, Undo2 } from "lucide-react";
import { clientMapNames, dynTaskLabels, openTaskNpc, pickEssence, saveTaskNpc } from "../elements/api";
import { bytes, count } from "../elements/format";
import type { TaskNpcFile, TaskNpcRow } from "../elements/types";
import { ValuePicker } from "./ValuePicker";

interface EditorRow extends TaskNpcRow {
  key: number;
}

interface History {
  states: EditorRow[][];
  cursor: number;
  /** Key of the edit that produced states[n + 1], for typing coalescing. */
  keys: string[];
}

export interface TaskNpcEditorState {
  loaded: boolean;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  path: string | null;
  rows: number;
}

export interface TaskNpcEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
  save: () => void;
  saveAs: () => void;
  undo: () => void;
  redo: () => void;
  add: () => void;
  cleanup: () => void;
}

interface Props {
  defaultPath?: string | null;
  active: boolean;
  onStateChange: (state: TaskNpcEditorState) => void;
  icon?: (pathId?: number | null) => string | undefined;
}

const PAGE_SIZE = 200;
const BACKUP_KEY = "jdide.taskNpc.backup";
const I16_MIN = -32768, I16_MAX = 32767, I32_MIN = -2147483648, I32_MAX = 2147483647;
const pathKey = (path: string) => path.replaceAll("/", "\\").toLowerCase();
const clamp = (value: string, min: number, max: number) => Math.max(min, Math.min(max, Math.trunc(Number(value)) || 0));
const same = (a: TaskNpcRow, b: TaskNpcRow) => a.id === b.id && a.map === b.map && a.x === b.x && a.y === b.y && a.z === b.z;

export const TaskNpcEditor = forwardRef<TaskNpcEditorHandle, Props>(function TaskNpcEditor({ defaultPath, active, onStateChange, icon }, ref) {
  const [file, setFile] = useState<TaskNpcFile | null>(null);
  const [history, setHistory] = useState<History>({ states: [], cursor: -1, keys: [] });
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query.trim().toLowerCase());
  const [onlyMissing, setOnlyMissing] = useState(false);
  const [onlyUnknown, setOnlyUnknown] = useState(false);
  const [page, setPage] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [backup, setBackup] = useState(() => { try { return localStorage.getItem(BACKUP_KEY) !== "0"; } catch { return true; } });
  const [selected, setSelected] = useState<number | null>(null);
  const [labels, setLabels] = useState<Record<string, string>>({});
  const [maps, setMaps] = useState<Map<number, string>>(new Map());
  const [picking, setPicking] = useState(false);
  const savedRows = useRef<EditorRow[] | null>(null);
  const savedByKey = useRef(new Map<number, EditorRow>());
  const nextKey = useRef(1);
  const autoOpened = useRef<string | null>(null);
  const backedUp = useRef(new Set<string>());
  const rows = history.cursor >= 0 ? history.states[history.cursor] : [];
  const dirty = !!file && rows !== savedRows.current;

  useEffect(() => { clientMapNames().then((list) => setMaps(new Map(list))).catch(() => {}); }, []);

  // Names of NPCs and monsters from the open elements.data.
  const labelIds = useCallback((ids: number[]) => {
    const wanted = [...new Set(ids)].filter((id) => id && !(String(id) in labels));
    if (wanted.length) dynTaskLabels(wanted, []).then((found) => setLabels((current) => ({ ...current, ...found.elements }))).catch(() => {});
  }, [labels]);

  const load = useCallback(async (path: string) => {
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const opened = await openTaskNpc(path);
      const loaded = opened.rows.map((row) => ({ ...row, key: nextKey.current++ }));
      savedRows.current = loaded;
      savedByKey.current = new Map(loaded.map((row) => [row.key, row]));
      setFile(opened);
      setHistory({ states: [loaded], cursor: 0, keys: [] });
      setQuery("");
      setPage(0);
      setSelected(null);
      setLabels({});
      dynTaskLabels(loaded.map((row) => row.id), []).then((found) => setLabels(found.elements)).catch(() => {});
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
    if (!dirty || window.confirm("Discard the unsaved task_npc.data changes?")) action();
  }, [dirty]);

  const choose = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, defaultPath: file?.path || defaultPath || undefined, title: "Open task_npc.data", filters: [{ name: "task_npc.data", extensions: ["data"] }] });
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
      if (states.length > 201) {
        states.shift();
        keys.shift();
        return { states, cursor: states.length - 1, keys };
      }
      return { states, cursor: current.cursor + 1, keys };
    });
    setNote(null);
  }, []);

  const change = (key: number, field: "id" | "map" | "x" | "y" | "z", value: string) => {
    const number = field === "id" ? clamp(value, 0, 0xffff_ffff) : field === "map" ? clamp(value, I32_MIN, I32_MAX) : clamp(value, I16_MIN, I16_MAX);
    commit(rows.map((row) => row.key === key ? { ...row, [field]: number } : row), `field:${key}:${field}`);
    if (field === "id") labelIds([number]);
  };

  /** Adds a record for an NPC or monster chosen in the picker (or selects its record). */
  const addId = (id: number) => {
    const existing = rows.find((row) => row.id === id);
    if (existing) {
      setSelected(existing.key);
      setNote(`ID ${id} already has a record.`);
      return;
    }
    const row: EditorRow = { key: nextKey.current++, id, map: 0, x: 0, y: 0, z: 0, pad: 0 };
    commit([...rows, row], `add:${row.key}`);
    labelIds([id]);
    setQuery(String(id));
    setOnlyMissing(false);
    setOnlyUnknown(false);
    setPage(0);
    setSelected(row.key);
  };
  const add = useCallback(() => { if (file) setPicking(true); }, [file]);

  const remove = (row: EditorRow) => {
    commit(rows.filter((candidate) => candidate.key !== row.key), `delete:${row.key}`);
    if (selected === row.key) setSelected(null);
  };

  /** Removes every record whose NPC or monster is not in the open elements.data (one undo step). */
  const cleanup = useCallback(async () => {
    if (!file || busy) return;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      // Checked again now, so an elements.data opened after this file counts.
      const found = (await dynTaskLabels([...new Set(rows.map((row) => row.id))], [])).elements;
      setLabels((current) => ({ ...current, ...found }));
      if (!Object.keys(found).length) {
        setError("None of these IDs is in an open elements.data. Open the matching elements.data first.");
        return;
      }
      const gone = rows.filter((row) => !(String(row.id) in found));
      if (!gone.length) {
        setNote("Every record names an NPC or monster of the open elements.data.");
        return;
      }
      const examples = gone.slice(0, 12).map((row) => row.id).join(", ");
      if (!window.confirm(`Remove ${count(gone.length)} record${gone.length === 1 ? "" : "s"} whose NPC or monster is not in the open elements.data?

IDs: ${examples}${gone.length > 12 ? ", …" : ""}

Undo (Ctrl+Z) brings them back until the file is saved.`)) return;
      commit(rows.filter((row) => String(row.id) in found), `cleanup:${Date.now()}`);
      if (selected !== null && gone.some((row) => row.key === selected)) setSelected(null);
      setOnlyUnknown(false);
      setNote(`Removed ${count(gone.length)} record${gone.length === 1 ? "" : "s"} not in elements.data. Ctrl+Z brings them back.`);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  }, [busy, commit, file, rows, selected]);

  const undo = useCallback(() => setHistory((current) => current.cursor > 0 ? { ...current, cursor: current.cursor - 1 } : current), []);
  const redo = useCallback(() => setHistory((current) => current.cursor + 1 < current.states.length ? { ...current, cursor: current.cursor + 1 } : current), []);

  const saveTo = useCallback(async (targetPath: string, replaceChanged = false) => {
    if (!file || busy) return;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const report = await saveTaskNpc({ openedPath: file.path, targetPath, token: file.token, rows: rows.map(({ id, map, x, y, z, pad }) => ({ id, map, x, y, z, pad })), backup: backup && !backedUp.current.has(pathKey(targetPath)), replaceChanged });
      savedRows.current = rows;
      savedByKey.current = new Map(rows.map((row) => [row.key, row]));
      if (report.backup) backedUp.current.add(pathKey(report.path));
      setHistory({ states: [rows], cursor: 0, keys: [] });
      setFile({ path: report.path, size: report.size, timeMark: report.timeMark, token: report.token, rows: rows.map(({ id, map, x, y, z, pad }) => ({ id, map, x, y, z, pad })) });
      setNote(`Saved ${count(report.rows)} records to ${report.path}.${report.backup ? ` Backup: ${report.backup}.` : ""} The client and the server each read their own copy: save the other one too.`);
    } catch (problem) {
      const message = String(problem).replace(/^Error: /, "");
      if (message.startsWith("CHANGED_ON_DISK:") && window.confirm(`${message.slice("CHANGED_ON_DISK:".length).trim()}\n\nReplace it with the open table?`)) {
        setBusy(false);
        await saveTo(targetPath, true);
        return;
      }
      setError(message);
    } finally {
      setBusy(false);
    }
  }, [backup, busy, file, rows]);

  const saveCurrent = useCallback(() => { if (file) void saveTo(file.path); }, [file, saveTo]);
  const saveAs = useCallback(async () => {
    if (!file || busy) return;
    const target = await save({ defaultPath: file.path, title: "Save task_npc.data as (for example the other copy)", filters: [{ name: "task_npc.data", extensions: ["data"] }] });
    if (typeof target === "string") await saveTo(target);
  }, [busy, file, saveTo]);

  useImperativeHandle(ref, () => ({ choose: () => void choose(), openPath: (path) => guard(() => void load(path)), save: saveCurrent, saveAs: () => void saveAs(), undo, redo, add, cleanup: () => void cleanup() }), [add, choose, cleanup, guard, load, redo, saveAs, saveCurrent, undo]);

  useEffect(() => onStateChange({ loaded: !!file, dirty, canUndo: history.cursor > 0, canRedo: history.cursor + 1 < history.states.length, path: file?.path ?? null, rows: rows.length }), [dirty, file, history.cursor, history.states.length, onStateChange, rows.length]);

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
  const elementsKnown = Object.keys(labels).length > 0;
  const mapName = (id: number) => id === 0 ? "No location" : maps.get(id);
  const visible = useMemo(() => rows.filter((row) => {
    if (onlyMissing && row.map !== 0) return false;
    if (onlyUnknown && elementsKnown && labels[String(row.id)]) return false;
    if (!deferredQuery) return true;
    return String(row.id).includes(deferredQuery) || (labels[String(row.id)] ?? "").toLowerCase().includes(deferredQuery) || (maps.get(row.map) ?? "").toLowerCase().includes(deferredQuery) || String(row.map) === deferredQuery;
  }).sort((a, b) => a.id - b.id), [deferredQuery, elementsKnown, labels, maps, onlyMissing, onlyUnknown, rows]);
  const pages = Math.max(1, Math.ceil(visible.length / PAGE_SIZE));
  useEffect(() => setPage((current) => Math.min(current, pages - 1)), [pages]);
  const shown = visible.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);
  const selectedRow = rows.find((row) => row.key === selected) ?? null;
  const located = useMemo(() => rows.filter((row) => row.map !== 0).length, [rows]);
  const mapOptions = useMemo(() => [...maps.entries()].sort((a, b) => a[0] - b[0]), [maps]);
  const search = useMemo(() => (text: string, number: number) => pickEssence("npc", text, number, null), []);

  if (!file) return <section className="path-data-pane empty">
    <div className="drop-card path-data-empty">
      <MapPin size={30} />
      <h2>Open task_npc.data</h2>
      <p className="muted">Where the quest tracker finds NPCs and monsters: their map and position. The client reads <span className="mono">element/data/task_npc.data</span> for the tracker and minimap links; the server reads its own copy (<span className="mono">QuestNPCInfo</span> in gs.conf) to teleport players to an NPC.</p>
      <button className="btn primary" onClick={() => void choose()} disabled={busy}><FolderOpen size={14} /> Choose task_npc.data…</button>
      {defaultPath && <button className="btn" onClick={() => void load(defaultPath)} disabled={busy}>Open the configured client's file</button>}
      {error && <div className="se-problems" role="alert">{error}</div>}
    </div>
  </section>;

  return <section className="path-data-pane">
    <header className="path-data-head">
      <div>
        <h2>Task NPC locations</h2>
        <div className="path-data-file-line">
          <span className="path-data-location mono muted small truncate" title={file.path}>{file.path}</span>
          <span className="path-data-badge"><b>Records:</b> {count(rows.length)}</span>
          <span className="path-data-badge" title="Records with a map"><b>Located:</b> {count(located)}</span>
          <span className="path-data-badge"><b>Size:</b> {bytes(file.size)}</span>
          <span className="path-data-badge" title="Saving sets it to now"><b>Time mark:</b> {new Date(file.timeMark * 1000).toLocaleString()}</span>
        </div>
      </div>
      {dirty && <span className="dirty-label">Unsaved changes</span>}
      <button className="btn" onClick={() => void choose()} disabled={busy}><FolderOpen size={13} /> Open…</button>
      <button className="btn" onClick={add} disabled={busy} title="Add a record for an NPC or monster of the open elements.data"><Plus size={13} /> Add NPC…</button>
      <button className="btn" onClick={() => void cleanup()} disabled={busy} title="Remove every record whose NPC or monster is not in the open elements.data"><Eraser size={13} /> Clean up…</button>
      <button className="icon-btn" onClick={undo} disabled={busy || history.cursor <= 0} title="Undo (Ctrl+Z)"><Undo2 size={15} /></button>
      <button className="icon-btn" onClick={redo} disabled={busy || history.cursor + 1 >= history.states.length} title="Redo (Ctrl+Y)"><Redo2 size={15} /></button>
      <button className="btn primary" onClick={saveCurrent} disabled={busy || !dirty}><Save size={13} /> Save</button>
    </header>
    <div className="path-data-tools">
      <label className="path-data-search"><Search size={14} /><input value={query} onChange={(event) => { setQuery(event.target.value); setPage(0); }} placeholder="ID, NPC name or map…" /></label>
      <label className="check"><input type="checkbox" checked={onlyMissing} onChange={(event) => { setOnlyMissing(event.target.checked); setPage(0); }} /> No location</label>
      <label className="check" title={elementsKnown ? undefined : "Open elements.data to check"}><input type="checkbox" checked={onlyUnknown} disabled={!elementsKnown} onChange={(event) => { setOnlyUnknown(event.target.checked); setPage(0); }} /> Not in elements.data</label>
      <span>{count(visible.length)} match{visible.length === 1 ? "" : "es"}</span>
      <span className="spacer" />
      <label className="check"><input type="checkbox" checked={backup} onChange={(event) => { setBackup(event.target.checked); try { localStorage.setItem(BACKUP_KEY, event.target.checked ? "1" : "0"); } catch { /* optional */ } }} /> Backup replaced file</label>
      <button className="btn small" onClick={() => void saveAs()} disabled={busy}>Save as…</button>
    </div>
    {(error || note) && <div className={error ? "path-data-message error" : "path-data-message"} role={error ? "alert" : "status"}>{error ? <AlertTriangle size={14} /> : <RotateCcw size={14} />}{error || note}</div>}
    <div className="path-data-body">
      <div className="path-data-table-wrap">
        <table className="path-data-table task-npc-table">
          <thead><tr><th>ID</th><th>NPC or monster</th><th>Map</th><th>X</th><th>Y</th><th>Z</th><th /></tr></thead>
          <tbody>{shown.map((row) => {
            const badId = row.id === 0 || (idCounts.get(row.id) ?? 0) > 1;
            const saved = savedByKey.current.get(row.key);
            const name = labels[String(row.id)];
            return <tr key={row.key} className={(row.key === selected ? "selected " : "") + (!saved || !same(saved, row) ? "changed" : "")} onClick={() => setSelected(row.key)}>
              <td><input className={badId ? "invalid mono" : "mono"} type="number" min={1} value={row.id || ""} onChange={(event) => change(row.key, "id", event.target.value)} aria-label={`ID ${row.id}`} /></td>
              <td className="truncate" title={name}>{name ? name.split(" › ").pop() : <span className="muted">{elementsKnown ? "not in elements.data" : "—"}</span>}</td>
              <td><span className="task-npc-map">
                <input className="mono" type="number" value={row.map} onChange={(event) => change(row.key, "map", event.target.value)} aria-label={`Map of ${row.id}`} />
                <span className={"truncate" + (row.map === 0 ? " muted" : "")} title={mapName(row.map)}>{mapName(row.map) ?? (maps.size ? "unknown map" : "")}</span>
              </span></td>
              {(["x", "y", "z"] as const).map((axis) => <td key={axis}><input className="mono" type="number" min={I16_MIN} max={I16_MAX} value={row[axis]} onChange={(event) => change(row.key, axis, event.target.value)} aria-label={`${axis.toUpperCase()} of ${row.id}`} /></td>)}
              <td><button className="icon-btn small danger" onClick={(event) => { event.stopPropagation(); remove(row); }} title={`Delete ID ${row.id} (undo brings it back)`}><Trash2 size={13} /></button></td>
            </tr>;
          })}</tbody>
        </table>
        {!shown.length && <div className="empty-note center">No records match.</div>}
      </div>
      <aside className="path-data-info">
        <h3>{selectedRow ? `ID ${selectedRow.id}` : "Record details"}</h3>
        {selectedRow ? <>
          <dl>
            <dt>NPC or monster</dt><dd>{labels[String(selectedRow.id)] ?? "(not in the open elements.data)"}</dd>
            <dt>Map</dt><dd>{selectedRow.map} · {mapName(selectedRow.map) ?? "unknown"}</dd>
            <dt>Position</dt><dd className="mono">{selectedRow.x}, {selectedRow.y}, {selectedRow.z}</dd>
          </dl>
          {(idCounts.get(selectedRow.id) ?? 0) > 1 && <p className="path-data-message error small">This ID has more than one record; the game keeps only the last. Saving refuses duplicates.</p>}
        </> : <p className="muted small">Select a row to inspect it.</p>}
        <div className="path-data-warning"><AlertTriangle size={14} /><span>The quest tracker and minimap links use the client's copy; "fly to NPC" teleports to the server's copy. Save both (Save as… writes the other one). Map 0 means no known location. Positions are whole world coordinates (x, height, z).</span></div>
        {!maps.size && <p className="muted small">Map names come from the game client's configs.pck; choose a client folder in Settings to see them.</p>}
        {mapOptions.length > 0 && <details className="small"><summary>Map IDs ({mapOptions.length})</summary><div className="task-npc-maps">{mapOptions.map(([id, name]) => <span key={id}><b className="mono">{id}</b> {name}</span>)}</div></details>}
      </aside>
    </div>
    <footer className="path-data-foot">
      <span>Rows {visible.length ? page * PAGE_SIZE + 1 : 0}–{Math.min((page + 1) * PAGE_SIZE, visible.length)} of {count(visible.length)}</span>
      <span className="spacer" />
      <button className="btn small" onClick={() => setPage(0)} disabled={page === 0}>First</button>
      <button className="btn small" onClick={() => setPage((value) => Math.max(0, value - 1))} disabled={page === 0}>Previous</button>
      <span className="muted small">Page {page + 1} of {pages}</span>
      <button className="btn small" onClick={() => setPage((value) => Math.min(pages - 1, value + 1))} disabled={page + 1 >= pages}>Next</button>
      <button className="btn small" onClick={() => setPage(pages - 1)} disabled={page + 1 >= pages}>Last</button>
    </footer>
    {picking && <ValuePicker search={search} icon={icon} onApply={async (value) => { const id = Number(value); if (!id) return "Choose an NPC or monster"; addId(id); return null; }} onClose={() => setPicking(false)} />}
  </section>;
});
