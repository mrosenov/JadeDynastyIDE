import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, ArrowDown, ArrowUp, Award, CircleAlert, Copy, FolderOpen, Info, Redo2, RotateCcw, Save, Search, Trash2, TriangleAlert, Undo2, X } from "lucide-react";
import { dynTaskLabels, openVipAward, pickEssence, saveVipAward, vipAwardLevelNames, vipAwardProblems } from "../elements/api";
import { count } from "../elements/format";
import { formatDuration } from "../elements/time";
import type { VipAward, VipAwardFile, VipAwardLevelNames, VipAwardProblem } from "../elements/types";
import { Field, NumberInput, TextInput } from "./DynTasksEditor";
import { ValuePicker } from "./ValuePicker";

interface Row extends VipAward {
  key: number;
}

interface History {
  states: Row[][];
  cursor: number;
  /** Key of the edit that produced states[n + 1], for typing coalescing. */
  keys: string[];
}

export interface VipAwardEditorState {
  loaded: boolean;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  path: string | null;
  awards: number;
}

export interface VipAwardEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
  save: () => void;
  saveAs: () => void;
  undo: () => void;
  redo: () => void;
  cloneSelected: () => void;
  deleteSelected: () => void;
  moveSelected: (step: -1 | 1) => void;
  openProblems: () => void;
}

interface Props {
  defaultPath?: string | null;
  active: boolean;
  onStateChange: (state: VipAwardEditorState) => void;
}

/** The tabs: what obtain type they show. */
type Tab = 0 | 1 | 2;
const TABS: { tab: Tab; label: string; hint: string }[] = [
  { tab: 0, label: "Daily", hint: "Claimed once a day" },
  { tab: 1, label: "Special", hint: "Claimed once (VIP: once per VIP period)" },
  { tab: 2, label: "VIP shop", hint: "Bought with the price in the VIP shop (newer builds)" },
];
const TYPES = ["Normal", "VIP", "VIP shop"];
const DURATIONS: [number, string][] = [[0, "Forever"], [3600, "1 hour"], [7200, "2 hours"], [43200, "12 hours"], [86400, "1 day"], [604800, "7 days"], [2592000, "30 days"]];
const BACKUP_KEY = "jdide.vipAward.backup";
const I32_MAX = 2147483647;
const pathKey = (path: string) => path.replaceAll("/", "\\").toLowerCase();
const fields = ({ key: _key, ...award }: Row): VipAward => award;
const same = (a: VipAward, b: VipAward) => a.id === b.id && a.name === b.name && a.itemId === b.itemId && a.count === b.count && a.awardType === b.awardType && a.level === b.level && a.obtainType === b.obtainType && a.expireTime === b.expireTime && a.price === b.price && a.extra.join() === b.extra.join();

export const VipAwardEditor = forwardRef<VipAwardEditorHandle, Props>(function VipAwardEditor({ defaultPath, active, onStateChange }, ref) {
  const [file, setFile] = useState<VipAwardFile | null>(null);
  const [history, setHistory] = useState<History>({ states: [], cursor: -1, keys: [] });
  const [tab, setTab] = useState<Tab>(0);
  /** The kind and level shown (null: every level of the tab). */
  const [filter, setFilter] = useState<{ type: number; level: number | null } | null>(null);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [backup, setBackup] = useState(() => { try { return localStorage.getItem(BACKUP_KEY) !== "0"; } catch { return true; } });
  const [labels, setLabels] = useState<Record<string, string>>({});
  const [names, setNames] = useState<VipAwardLevelNames | null>(null);
  const [picking, setPicking] = useState(false);
  const [problems, setProblems] = useState<VipAwardProblem[] | null>(null);
  const [problemsOpen, setProblemsOpen] = useState(false);
  const savedRows = useRef<Row[] | null>(null);
  const savedByKey = useRef(new Map<number, Row>());
  const nextKey = useRef(1);
  const autoOpened = useRef<string | null>(null);
  const backedUp = useRef(new Set<string>());
  const rows = history.cursor >= 0 ? history.states[history.cursor] : [];
  const dirty = !!file && rows !== savedRows.current;
  const newer = !!file && file.recordSize > 156;

  useEffect(() => { vipAwardLevelNames().then(setNames).catch(() => {}); }, []);

  const labelIds = useCallback((ids: number[]) => {
    const wanted = [...new Set(ids)].filter((id) => id && !(String(id) in labels));
    if (wanted.length) dynTaskLabels(wanted, []).then((found) => setLabels((current) => ({ ...current, ...found.elements }))).catch(() => {});
  }, [labels]);
  const itemName = (id: number) => labels[String(id)]?.split(" › ").pop();

  const load = useCallback(async (path: string) => {
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const opened = await openVipAward(path);
      const loaded = opened.awards.map((award) => ({ ...award, key: nextKey.current++ }));
      savedRows.current = loaded;
      savedByKey.current = new Map(loaded.map((row) => [row.key, row]));
      setFile(opened);
      setHistory({ states: [loaded], cursor: 0, keys: [] });
      setSelected(null);
      setFilter(null);
      setTab(0);
      setProblems(null);
      setLabels({});
      dynTaskLabels([...new Set(loaded.map((row) => row.itemId))], []).then((found) => setLabels(found.elements)).catch(() => {});
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
    if (!dirty || window.confirm("Discard the unsaved vipaward.data changes?")) action();
  }, [dirty]);
  const choose = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, defaultPath: file?.path || defaultPath || undefined, title: "Open vipaward.data (VIPAward.data)", filters: [{ name: "vipaward.data", extensions: ["data"] }] });
    if (typeof picked === "string") guard(() => void load(picked));
  }, [defaultPath, file?.path, guard, load]);

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
  const undo = useCallback(() => setHistory((current) => current.cursor > 0 ? { ...current, cursor: current.cursor - 1 } : current), []);
  const redo = useCallback(() => setHistory((current) => current.cursor + 1 < current.states.length ? { ...current, cursor: current.cursor + 1 } : current), []);

  const change = useCallback((key: number, update: Partial<VipAward>, label: string) => {
    commit(rows.map((row) => (row.key === key ? { ...row, ...update } : row)), `${label}:${key}`);
    if (update.itemId) labelIds([update.itemId]);
  }, [commit, labelIds, rows]);

  // The awards shown: the tab's obtain type, the chosen kind and level, the search; in file order (the client's).
  const needle = query.trim().toLowerCase();
  const shown = useMemo(() => rows.filter((row) => {
    if (row.obtainType !== tab) return false;
    if (filter && (row.awardType !== filter.type || (filter.level !== null && row.level !== filter.level))) return false;
    if (!needle) return true;
    return String(row.id) === needle || String(row.itemId).includes(needle) || row.name.toLowerCase().includes(needle) || (labels[String(row.itemId)] ?? "").toLowerCase().includes(needle);
  }), [filter, labels, needle, rows, tab]);
  const selectedRow = rows.find((row) => row.key === selected) ?? null;

  const cloneSelected = useCallback(() => {
    if (!selectedRow) return;
    const id = Math.max(0, ...rows.map((row) => row.id)) + 1;
    const copy: Row = { ...selectedRow, key: nextKey.current++, id };
    const at = rows.findIndex((row) => row.key === selectedRow.key);
    commit([...rows.slice(0, at + 1), copy, ...rows.slice(at + 1)], `clone:${copy.key}`);
    setSelected(copy.key);
    setNote(`Award ${id} is a copy of award ${selectedRow.id} (IDs are unique; the next free one was taken).`);
  }, [commit, rows, selectedRow]);
  const deleteSelected = useCallback(() => {
    if (!selectedRow) return;
    if (!window.confirm(`Delete award ${selectedRow.id} (${itemName(selectedRow.itemId) ?? (selectedRow.name || `item ${selectedRow.itemId}`)})? Undo brings it back.`)) return;
    const at = shown.findIndex((row) => row.key === selectedRow.key);
    commit(rows.filter((row) => row.key !== selectedRow.key), `delete:${selectedRow.key}`);
    setSelected(shown[at + 1]?.key ?? shown[at - 1]?.key ?? null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [commit, rows, selectedRow, shown]);
  /** Moves the selected award before or after its neighbour in the list shown (the client lists a level in file order). */
  const moveSelected = useCallback((step: -1 | 1) => {
    if (!selectedRow) return;
    const at = shown.findIndex((row) => row.key === selectedRow.key);
    const neighbour = shown[at + step];
    if (at < 0 || !neighbour) return;
    const without = rows.filter((row) => row.key !== selectedRow.key);
    const target = without.findIndex((row) => row.key === neighbour.key) + (step > 0 ? 1 : 0);
    commit([...without.slice(0, target), selectedRow, ...without.slice(target)], `move:${selectedRow.key}:${Date.now()}`);
  }, [commit, rows, selectedRow, shown]);

  const checkProblems = useCallback(async () => {
    if (!file) return;
    setProblems(null);
    setProblemsOpen(true);
    try {
      setProblems(await vipAwardProblems(rows.map(fields), file.recordSize));
    } catch (problem) {
      setProblemsOpen(false);
      setError(String(problem).replace(/^Error: /, ""));
    }
  }, [file, rows]);

  const saveTo = useCallback(async (targetPath: string, replaceChanged = false) => {
    if (!file || busy) return;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const awards = rows.map(fields);
      const report = await saveVipAward({ openedPath: file.path, targetPath, token: file.token, recordSize: file.recordSize, awards, backup: backup && !backedUp.current.has(pathKey(targetPath)), replaceChanged });
      savedRows.current = rows;
      savedByKey.current = new Map(rows.map((row) => [row.key, row]));
      if (report.backup) backedUp.current.add(pathKey(report.path));
      setHistory({ states: [rows], cursor: 0, keys: [] });
      setFile({ ...file, path: report.path, size: report.size, timestamp: report.timestamp, token: report.token, awards });
      setNote(`Saved ${count(report.awards)} awards to ${report.path}.${report.backup ? ` Backup: ${report.backup}.` : ""} The client and the server need the same awards (claims send the award and item IDs): copy it to the other side too.`);
    } catch (problem) {
      const message = String(problem).replace(/^Error: /, "");
      if (message.startsWith("CHANGED_ON_DISK:") && window.confirm(`${message.slice("CHANGED_ON_DISK:".length).trim()}\n\nReplace it with the open awards?`)) {
        setBusy(false);
        await saveTo(targetPath, true);
        return;
      }
      setError(message);
    } finally {
      setBusy(false);
    }
  }, [backup, busy, file, rows]);
  /** The server's rules before writing: an error there stops it from starting (gs exits with -8). */
  const checkedSave = useCallback(async (targetPath: string) => {
    if (!file || busy) return;
    let found: VipAwardProblem[] = [];
    try {
      found = await vipAwardProblems(rows.map(fields), file.recordSize);
    } catch {
      // Checking is a help; saving still works without it.
    }
    const errors = found.filter((problem) => problem.severity === "error");
    if (errors.length) {
      const lines = errors.slice(0, 8).map((problem) => `• ${problem.index !== null && rows[problem.index] ? `Award ${rows[problem.index].id}: ` : ""}${problem.message}`).join("\n");
      const more = errors.length > 8 ? `\n… and ${errors.length - 8} more` : "";
      if (!window.confirm(`The server would refuse to start with this file:\n\n${lines}${more}\n\nSave anyway?`)) {
        setProblems(found);
        setProblemsOpen(true);
        return;
      }
    }
    await saveTo(targetPath);
    if (!Object.keys(labels).length) setNote((current) => `${current ?? ""} Items were not checked: open the server's elements.data so stack limits are checked too (an item that stacks to 1 needs count 1, or the server does not start).`.trim());
  }, [busy, file, labels, rows, saveTo]);
  const saveCurrent = useCallback(() => { if (file && dirty) void checkedSave(file.path); }, [checkedSave, dirty, file]);
  const saveAs = useCallback(async () => {
    if (!file || busy) return;
    const target = await save({ defaultPath: file.path, title: "Save vipaward.data as (for example the server's copy)", filters: [{ name: "vipaward.data", extensions: ["data"] }] });
    if (typeof target === "string") await checkedSave(target);
  }, [busy, checkedSave, file]);

  useImperativeHandle(ref, () => ({ choose: () => void choose(), openPath: (path) => guard(() => void load(path)), save: saveCurrent, saveAs: () => void saveAs(), undo, redo, cloneSelected, deleteSelected, moveSelected, openProblems: () => void checkProblems() }), [checkProblems, choose, cloneSelected, deleteSelected, guard, load, moveSelected, redo, saveAs, saveCurrent, undo]);
  useEffect(() => onStateChange({ loaded: !!file, dirty, canUndo: history.cursor > 0, canRedo: history.cursor + 1 < history.states.length, path: file?.path ?? null, awards: rows.length }), [dirty, file, history.cursor, history.states.length, onStateChange, rows.length]);

  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      const mod = event.ctrlKey || event.metaKey;
      const typing = event.target instanceof HTMLElement && /^(INPUT|TEXTAREA|SELECT)$/.test(event.target.tagName);
      const key = event.key.toLowerCase();
      if (event.key === "Escape" && problemsOpen) { event.preventDefault(); setProblemsOpen(false); return; }
      if (mod && event.shiftKey && key === "m") { event.preventDefault(); event.stopImmediatePropagation(); void checkProblems(); return; }
      if (mod && ["o", "s", "z", "y", "d"].includes(key)) {
        if (typing && (key === "z" || key === "y")) return;
        event.preventDefault();
        event.stopImmediatePropagation();
        if (key === "o") void choose();
        else if (key === "s") event.shiftKey ? void saveAs() : saveCurrent();
        else if (key === "z") event.shiftKey ? redo() : undo();
        else if (key === "y") redo();
        else cloneSelected();
      } else if (event.key === "Delete" && !typing) {
        event.preventDefault();
        deleteSelected();
      } else if (event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown") && !typing) {
        event.preventDefault();
        moveSelected(event.key === "ArrowUp" ? -1 : 1);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [active, checkProblems, choose, cloneSelected, deleteSelected, moveSelected, problemsOpen, redo, saveAs, saveCurrent, undo]);

  const levelName = (type: number, level: number) => {
    const list = type === 0 ? names?.normal : names?.vip;
    return list?.[level - 1] ?? `Level ${level}`;
  };
  const vipLevels = newer ? 8 : 6;
  const countIn = (type: number, level: number | null) => rows.filter((row) => row.obtainType === tab && row.awardType === type && (level === null || row.level === level)).length;
  const kinds = tab === 2 ? [2] : [0, 1];
  const search = useMemo(() => (text: string, page: number) => pickEssence("item", text, page, selectedRow?.itemId || null), [selectedRow?.itemId]);

  if (!file) return <section className="path-data-pane empty">
    <div className="drop-card path-data-empty">
      <Award size={30} />
      <h2>Open vipaward.data</h2>
      <p className="muted">The daily and special awards of the VIP award window (and the VIP shop of newer builds). The client reads <span className="mono">element/data/VIPAward.data</span>; the server its own copy (<span className="mono">VipAwardData</span> in gs.conf). Both need the same awards.</p>
      <button className="btn primary" onClick={() => void choose()} disabled={busy}><FolderOpen size={14} /> Choose vipaward.data…</button>
      {defaultPath && <button className="btn" onClick={() => void load(defaultPath)} disabled={busy}>Open the configured client's file</button>}
      {error && <div className="se-problems" role="alert">{error}</div>}
    </div>
  </section>;

  return <section className="dyn-tasks-pane" aria-busy={busy}>
    <header className="tasks-head">
      <div><h2>VIP awards {dirty && <span className="tag warn">unsaved</span>}</h2><div className="tasks-file-line">
        <span className="mono truncate" title={file.path}>{file.path.split(/[\\/]/).slice(-3).join("/")}</span>
        <span className="path-data-badge"><b>Awards:</b> {count(rows.length)}</span>
        <span className="path-data-badge" title={newer ? "Newer builds (HDN, Reborn, 2018) add 16 bytes: the VIP shop price, purchase limit and two unknown values" : "The source's record (2013–2015 builds)"}><b>Record:</b> {file.recordSize} bytes{newer ? " (newer build)" : ""}</span>
        <span className="path-data-badge" title="The export time; nothing compares it. Saving sets it to now."><b>Timestamp:</b> {new Date(file.timestamp * 1000).toLocaleString()}</span>
      </div></div>
      <button className="btn" onClick={undo} disabled={busy || history.cursor <= 0} title="Undo (Ctrl+Z)"><Undo2 size={14} /></button>
      <button className="btn" onClick={redo} disabled={busy || history.cursor + 1 >= history.states.length} title="Redo (Ctrl+Y)"><Redo2 size={14} /></button>
      <button className="btn" onClick={() => void checkProblems()} title="Check problems (Ctrl+Shift+M)"><CircleAlert size={14} /> Problems</button>
      <button className="btn" onClick={() => void choose()} disabled={busy}><FolderOpen size={14} /> Open…</button>
      <button className="btn primary" onClick={saveCurrent} disabled={busy || !dirty}><Save size={14} /> Save</button>
    </header>
    {(error || note) && <div className={error ? "path-data-message error" : "path-data-message ok"} role={error ? "alert" : "status"}>{error ? <AlertTriangle size={14} /> : <RotateCcw size={14} />} {error || note} <button className="link" onClick={() => { setError(null); setNote(null); }}>Dismiss</button></div>}
    <div className="vip-tabs">
      {TABS.filter((entry) => entry.tab < 2 || newer).map((entry) => <button key={entry.tab} className={"btn small" + (tab === entry.tab ? " active" : "")} title={entry.hint} onClick={() => { setTab(entry.tab); setFilter(null); setSelected(null); }}>{entry.label} <span className="muted">{count(rows.filter((row) => row.obtainType === entry.tab).length)}</span></button>)}
      <span className="spacer" />
      <label className="check"><input type="checkbox" checked={backup} onChange={(event) => { setBackup(event.target.checked); try { localStorage.setItem(BACKUP_KEY, event.target.checked ? "1" : "0"); } catch { /* optional */ } }} /> Backup replaced file</label>
      <button className="btn small" onClick={() => void saveAs()} disabled={busy}>Save as…</button>
    </div>
    <div className="gshop-body">
      <aside className="gshop-tree">
        <button className={"dyn-task-row" + (filter === null ? " selected" : "")} onClick={() => setFilter(null)}><b>All {tab === 2 ? "VIP shop items" : TABS[tab].label.toLowerCase()}</b> <span className="muted small">{count(rows.filter((row) => row.obtainType === tab).length)}</span></button>
        {kinds.map((type) => <div key={type}>
          <button className={"dyn-task-row" + (filter?.type === type && filter.level === null ? " selected" : "")} onClick={() => setFilter({ type, level: null })}><span>{TYPES[type]}</span> <span className="muted small">{count(countIn(type, null))}</span></button>
          {Array.from({ length: type === 0 ? 8 : vipLevels }, (_, index) => index + 1).map((level) => <button key={level} className={"dyn-task-row gshop-sub" + (filter?.type === type && filter.level === level ? " selected" : "")} onClick={() => setFilter({ type, level })} title={`Level ${level}`}><span>{levelName(type, level)}</span> <span className="muted small">{count(countIn(type, level))}</span></button>)}
        </div>)}
        {names && !names.fromClient && <p className="muted small vip-names-note">Level names are the English client's; set the client folder in Settings to use yours.</p>}
      </aside>
      <aside className="dyn-task-list gshop-list">
        <div className="dyn-task-search"><Search size={13} /><input value={query} placeholder="Award ID, item ID or name" onChange={(event) => setQuery(event.target.value)} />{query && <button className="icon-btn small" onClick={() => setQuery("")}><X size={12} /></button>}</div>
        <div className="dyn-task-rows" role="listbox">
          {shown.map((row) => {
            const saved = savedByKey.current.get(row.key);
            return <button key={row.key} role="option" aria-selected={row.key === selected} className={"dyn-task-row vip-row" + (row.key === selected ? " selected" : "")} onClick={() => setSelected(row.key)}>
              <span className="mono muted">{row.id}</span>
              <span className="gshop-row-text"><span className="truncate">{itemName(row.itemId) ?? (row.name || <span className="muted">item {row.itemId}</span>)}{row.count > 1 && <span className="muted"> ×{count(row.count)}</span>}</span><span className="muted small truncate">{TYPES[row.awardType] ?? `type ${row.awardType}`} · {levelName(row.awardType === 0 ? 0 : 1, row.level)}{row.expireTime ? ` · ${formatDuration(row.expireTime)}` : ""}{row.awardType === 2 && row.price !== null ? ` · ${count(row.price)}` : ""}</span></span>
              {!saved || !same(saved, row) ? <span className="changed-dot" title="Changed" /> : <span />}
            </button>;
          })}
          {!shown.length && <div className="empty-note">No awards here.</div>}
        </div>
        <footer>
          <button className="btn small" onClick={cloneSelected} disabled={!selectedRow} title="Copy the selected award with the next free ID (Ctrl+D)"><Copy size={12} /> Clone</button>
          <button className="btn small" onClick={deleteSelected} disabled={!selectedRow} title="Delete (Del)"><Trash2 size={12} /></button>
          <button className="btn small" onClick={() => moveSelected(-1)} disabled={!selectedRow} title="Move up (Alt+↑): the client lists a level's awards in file order"><ArrowUp size={12} /></button>
          <button className="btn small" onClick={() => moveSelected(1)} disabled={!selectedRow} title="Move down (Alt+↓)"><ArrowDown size={12} /></button>
          <span className="spacer" />
          <span className="muted small">{count(shown.length)}</span>
        </footer>
      </aside>
      <div className="dyn-task-form-scroll">
        {!selectedRow ? <div className="empty-note">Select an award. The client shows a level's awards in this order, with the item's own name from elements.data, the count, the duration and a daily or once mark.</div> : <>
          <div className="dyn-task-title"><h3>Award {selectedRow.id}</h3><span className="muted small">{itemName(selectedRow.itemId) ?? ""}</span></div>
          <div className="vip-form">
            <Field label="Award ID" hint="Unique; the client sends it to claim the award, so the server's file must have the same"><NumberInput min={1} max={I32_MAX} value={selectedRow.id} onCommit={(id) => change(selectedRow.key, { id }, "id")} /></Field>
            <Field label="Item" hint="The item given (the client shows its name and icon)"><span className="dyn-item-cell"><NumberInput min={1} max={I32_MAX} value={selectedRow.itemId} onCommit={(itemId) => change(selectedRow.key, { itemId }, "item")} /><button className="icon-btn small" title="Choose from elements.data" onClick={() => setPicking(true)}><Search size={12} /></button><span className="dyn-label truncate">{labels[String(selectedRow.itemId)]?.split(" › ").pop() ?? <span className="muted">{Object.keys(labels).length ? "not in elements.data" : "open elements.data for names"}</span>}</span></span></Field>
            <Field label="Count" hint="An item that stacks to 1 must have count 1, or the server does not start (error -8)"><NumberInput min={1} max={I32_MAX} value={selectedRow.count} onCommit={(value) => change(selectedRow.key, { count: value }, "count")} /></Field>
            <Field label="Name" hint="Stored with the award; the client shows the item's own name instead"><span className="vip-name"><TextInput value={selectedRow.name} max={64} onCommit={(name) => change(selectedRow.key, { name }, "name")} />{itemName(selectedRow.itemId) && itemName(selectedRow.itemId) !== selectedRow.name && <button className="btn small" onClick={() => change(selectedRow.key, { name: itemName(selectedRow.itemId)! }, "name")}>Use the item's name</button>}</span></Field>
            <Field label="Kind"><select className="task-form-select" value={selectedRow.awardType} onChange={(event) => { const awardType = Number(event.target.value); change(selectedRow.key, awardType === 2 ? { awardType, obtainType: 2 } : selectedRow.obtainType === 2 ? { awardType, obtainType: 0 } : { awardType }, "type"); }}>
              {TYPES.map((label, value) => (value < 2 || newer) && <option key={value} value={value}>{label}</option>)}
              {selectedRow.awardType > (newer ? 2 : 1) && <option value={selectedRow.awardType}>Unknown ({selectedRow.awardType})</option>}
            </select></Field>
            <Field label="Level" hint={selectedRow.awardType === 0 ? "Normal awards go by player level (and rebirth): only the player's own band" : "VIP awards go by VIP level: daily ones only at the player's level, special ones at it or below"}><select className="task-form-select" value={selectedRow.level} onChange={(event) => change(selectedRow.key, { level: Number(event.target.value) }, "level")}>
              {Array.from({ length: selectedRow.awardType === 0 ? 8 : vipLevels }, (_, index) => index + 1).map((level) => <option key={level} value={level}>{level} · {levelName(selectedRow.awardType === 0 ? 0 : 1, level)}</option>)}
              {(selectedRow.level < 1 || selectedRow.level > (selectedRow.awardType === 0 ? 8 : vipLevels)) && <option value={selectedRow.level}>{selectedRow.level} (out of range)</option>}
            </select></Field>
            <Field label="Claimed"><select className="task-form-select" value={selectedRow.obtainType} disabled={selectedRow.awardType === 2} onChange={(event) => change(selectedRow.key, { obtainType: Number(event.target.value) }, "obtain")}>
              <option value={0}>Daily</option><option value={1}>Once (special)</option>{newer && <option value={2}>Bought (VIP shop)</option>}
              {selectedRow.obtainType > (newer ? 2 : 1) && <option value={selectedRow.obtainType}>Unknown ({selectedRow.obtainType})</option>}
            </select></Field>
            <Field label="Duration" hint="How long the item lasts once given (seconds; 0 = forever)"><span className="vip-duration"><NumberInput min={0} max={I32_MAX} value={selectedRow.expireTime} onCommit={(expireTime) => change(selectedRow.key, { expireTime }, "expire")} /><select className="task-form-select" value={DURATIONS.some(([seconds]) => seconds === selectedRow.expireTime) ? selectedRow.expireTime : -1} onChange={(event) => { const expireTime = Number(event.target.value); if (expireTime >= 0) change(selectedRow.key, { expireTime }, "expire"); }}>
              {DURATIONS.map(([seconds, label]) => <option key={seconds} value={seconds}>{label}</option>)}
              {!DURATIONS.some(([seconds]) => seconds === selectedRow.expireTime) && <option value={-1}>{formatDuration(selectedRow.expireTime)}</option>}
            </select></span></Field>
            {selectedRow.price !== null && (selectedRow.awardType === 2 || !!selectedRow.price || selectedRow.extra.some(Boolean)) && <>
              <Field label="Price" hint="VIP shop: the price shown on the item (价格); a float in the file"><NumberInput float min={0} max={1e9} value={selectedRow.price} onCommit={(price) => change(selectedRow.key, { price }, "price")} /></Field>
              {selectedRow.extra.length > 0 && <Field label="Purchase limit" hint="VIP shop: how many times it can be bought (限次); 1–3 in Reborn, 0 in HDN"><NumberInput min={-2147483648} max={I32_MAX} value={selectedRow.extra[0]} onCommit={(value) => change(selectedRow.key, { extra: selectedRow.extra.map((entry, index) => (index === 0 ? value : entry)) }, "limit")} /></Field>}
            </>}
            {selectedRow.extra.length > 1 && <details className="vip-more"><summary className="muted small">Unknown values of newer builds ({selectedRow.extra.slice(1).every((value) => value === 0) ? "all 0" : "set"})</summary>
              {selectedRow.extra.slice(1).map((value, index) => <Field key={index} label={`Unknown ${index + 1}`} hint="0 in every sample"><NumberInput min={-2147483648} max={I32_MAX} value={value} onCommit={(next) => change(selectedRow.key, { extra: selectedRow.extra.map((entry, position) => (position === index + 1 ? next : entry)) }, `extra${index}`)} /></Field>)}
            </details>}
          </div>
          <div className="path-data-warning"><Info size={14} /><span>{selectedRow.awardType === 0 ? "Normal awards: a player sees the awards of their own level band (bands 5–8 after rebirth)." : selectedRow.awardType === 1 ? "VIP awards: daily ones at the player's VIP level only, special ones at it or any level below, once per VIP period. Nothing can be claimed between 0:00 and 6:00." : "VIP shop: bought for the price; the limit says how often."} Claims send the award and item IDs, so the client and the server need the same file.</span></div>
        </>}
      </div>
    </div>
    {picking && selectedRow && <ValuePicker search={search} onApply={async (value) => { const itemId = Number(value); if (!itemId) return "Choose an item"; change(selectedRow.key, { itemId }, "item"); setPicking(false); return null; }} onClose={() => setPicking(false)} />}
    {problemsOpen && (() => {
      const errors = (problems ?? []).filter((problem) => problem.severity === "error").length;
      const warnings = (problems ?? []).length - errors;
      return <div className="modal-backdrop" onMouseDown={() => setProblemsOpen(false)}>
        <div className="modal npcgen-problems-dialog" role="dialog" aria-label="Problems" onMouseDown={(event) => event.stopPropagation()}>
          <header className="modal-head"><h3>Problems</h3>{problems && <span className="muted small">{count(errors)} error{errors === 1 ? "" : "s"} · {count(warnings)} warning{warnings === 1 ? "" : "s"}</span>}<span className="spacer" /><button className="btn small" onClick={() => void checkProblems()} disabled={!problems}>Check again</button><button className="icon-btn" onClick={() => setProblemsOpen(false)} aria-label="Close"><X size={16} /></button></header>
          <div className="npcgen-problems-list">
            {!problems ? <div className="empty-note">Checking…</div> : !problems.length ? <div className="empty-note">No problems: the server accepts every award.</div> : problems.map((problem, index) => {
              const row = problem.index !== null ? rows[problem.index] : null;
              return <button key={index} className={"npcgen-problem " + problem.severity} onClick={() => { if (row) { setTab((row.obtainType <= 2 ? row.obtainType : 0) as Tab); setFilter(null); setQuery(""); setSelected(row.key); setProblemsOpen(false); } }}>
                {problem.severity === "error" ? <CircleAlert size={14} /> : <TriangleAlert size={14} />}
                <span>{row ? <b>Award {row.id}</b> : <b>File</b>} {row && <span className="muted">{itemName(row.itemId) ?? row.name}</span>} {problem.message}</span>
              </button>;
            })}
          </div>
          <footer className="modal-foot muted small">Rules from the server source (playervipaward.cpp): it refuses to start on any error. {Object.keys(labels).length ? "Items are checked against the open elements.data." : "Open the matching elements.data to check the items too."}</footer>
        </div>
      </div>;
    })()}
  </section>;
});
