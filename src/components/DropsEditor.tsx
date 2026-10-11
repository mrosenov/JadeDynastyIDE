import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, ArrowDown, ArrowDownWideNarrow, ArrowUp, CircleAlert, Copy, Dices, FolderOpen, Info, Loader2, Plus, Redo2, Save, Scale, Search, Trash2, TriangleAlert, Undo2, X } from "lucide-react";
import { dynTaskLabels, extraDropsProblems, openExtraDrops, pickEssence, readExtraDrops, saveExtraDrops } from "../elements/api";
import { bytes, count } from "../elements/format";
import type { DropFile, DropProblem, DropTable } from "../elements/types";
import { NumberInput, TextInput } from "./DynTasksEditor";
import { ValuePicker } from "./ValuePicker";
import { DropsCompare } from "./DropsCompare";

export interface DropsEditorState {
  loaded: boolean;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  path: string | null;
  tables: number;
}

export interface DropsEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
  save: () => void;
  saveAs: () => void;
  undo: () => void;
  redo: () => void;
  cloneSelected: () => void;
  deleteSelected: () => void;
  openProblems: () => void;
  /** Shows the tables of a monster or item ID. */
  showId: (id: number) => void;
  compare: () => void;
  /** Converts the file to version 1 (every server) or 2 (zx_18 server). */
  setVersion: (version: number) => void;
}

interface Props {
  active: boolean;
  /** elements.data is open (names of monsters and items, ID checks). */
  elementsPath: string | null;
  onStateChange: (state: DropsEditorState) => void;
}

interface History {
  states: DropFile[];
  cursor: number;
  keys: string[];
}

const LAST_KEY = "jdide.drops.lastPath";
const BACKUP_KEY = "jdide.drops.backup";
const TYPES: Record<number, string> = { 1: "Replace the monster's drops", 2: "Add to the monster's drops" };
const messageOf = (problem: unknown) => String(problem).replace(/^Error: /, "");
/** A sum as the server adds it (f32, in order). */
const f32Sum = (values: number[]) => values.reduce((sum, value) => Math.fround(sum + value), 0);
const usedItems = (table: DropTable) => { const end = table.items.findIndex((item) => item.id === 0); return end < 0 ? table.items : table.items.slice(0, end); };
const percent = (value: number) => `${Math.round(value * 1_000_000) / 10_000}%`;
const exact = (sum: number) => Math.abs(1 - sum) <= 1e-5;
const shortName = (label?: string) => label?.split(" › ").pop();

/** Scales chances to exactly 100% as the server adds them (f32); the largest takes the rounding rest. */
function scaled(values: number[]): number[] | null {
  const total = values.reduce((sum, value) => sum + Math.max(0, value), 0);
  if (!(total > 0)) return null;
  const out = values.map((value) => Math.fround(Math.max(0, value) / total));
  const largest = out.reduce((best, value, index) => value > out[best] ? index : best, 0);
  out[largest] = Math.fround(out[largest] + (1 - f32Sum(out)));
  return out;
}

/** A chance as a percentage input (applies on Enter or leaving the field). */
function PercentInput({ value, onCommit, invalid }: { value: number; onCommit: (value: number) => void; invalid?: boolean }) {
  const shown = String(Math.round(value * 1_000_000) / 10_000);
  const [draft, setDraft] = useState(shown);
  useEffect(() => setDraft(shown), [shown]);
  const commit = () => {
    const number = Number(draft.trim().replace(/%$/, ""));
    if (!draft.trim() || !Number.isFinite(number) || number < 0 || number > 100) { setDraft(shown); return; }
    const next = Math.fround(number / 100);
    if (next !== value) onCommit(next);
  };
  return <span className="ai-percent"><input className={"dyn-input mono" + (invalid ? " invalid" : "")} value={draft} onChange={(event) => setDraft(event.target.value)} onBlur={commit} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); if (event.key === "Escape") setDraft(shown); }} />%</span>;
}

/** extra_drops.sev: the server's extra drop tables for monsters. */
export const DropsEditor = forwardRef<DropsEditorHandle, Props>(function DropsEditor({ active, elementsPath, onStateChange }, ref) {
  const [meta, setMeta] = useState<{ path: string; size: number; token: string } | null>(null);
  const [history, setHistory] = useState<History>({ states: [], cursor: -1, keys: [] });
  const [selected, setSelected] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [backup, setBackup] = useState(() => { try { return localStorage.getItem(BACKUP_KEY) !== "0"; } catch { return true; } });
  const [problems, setProblems] = useState<DropProblem[] | null>(null);
  const [problemsOpen, setProblemsOpen] = useState(false);
  const [names, setNames] = useState<Record<string, string>>({});
  const [picking, setPicking] = useState<{ kind: "monster" | "item"; current: number; apply: (id: number) => void } | null>(null);
  const [monsterIds, setMonsterIds] = useState("");
  const [other, setOther] = useState<{ path: string; file: DropFile } | null>(null);
  const saved = useRef<DropFile | null>(null);
  const autoOpened = useRef(false);
  const backedUp = useRef(new Set<string>());

  const file = history.cursor >= 0 ? history.states[history.cursor] : null;
  const dirty = !!file && file !== saved.current;

  const load = useCallback(async (path: string) => {
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const opened = await openExtraDrops(path);
      saved.current = opened.file;
      setMeta({ path: opened.path, size: opened.size, token: opened.token });
      setHistory({ states: [opened.file], cursor: 0, keys: [] });
      setSelected(opened.file.tables.length ? 0 : null);
      setQuery("");
      try { localStorage.setItem(LAST_KEY, opened.path); } catch { /* optional */ }
    } catch (problem) {
      setError(messageOf(problem));
    } finally {
      setBusy(false);
    }
  }, []);
  // The file opened last time.
  useEffect(() => {
    if (autoOpened.current) return;
    autoOpened.current = true;
    let last: string | null = null;
    try { last = localStorage.getItem(LAST_KEY); } catch { /* optional */ }
    if (last) void load(last);
  }, [load]);
  const guard = useCallback((action: () => void) => {
    if (!dirty || window.confirm("Discard the unsaved extra_drops.sev changes?")) action();
  }, [dirty]);
  const choose = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, defaultPath: meta?.path, title: "Open extra_drops.sev (server config)", filters: [{ name: "extra_drops.sev", extensions: ["sev"] }, { name: "All files", extensions: ["*"] }] });
    if (typeof picked === "string") guard(() => void load(picked));
  }, [guard, load, meta?.path]);

  // Names of every monster and item the tables name (elements.data).
  useEffect(() => {
    if (!file || !elementsPath) { setNames({}); return; }
    const ids = new Set<number>();
    for (const table of file.tables) { table.monsters.forEach((id) => ids.add(id)); table.items.forEach((item) => item.id && ids.add(item.id)); }
    const missing = [...ids].filter((id) => !(String(id) in names));
    if (!missing.length) return;
    dynTaskLabels(missing, []).then((found) => setNames((current) => ({ ...current, ...Object.fromEntries(missing.map((id) => [String(id), found.elements[String(id)] ?? ""])) }))).catch(() => {});
    // names is read to skip known IDs only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [elementsPath, file]);
  useEffect(() => setNames({}), [elementsPath]);
  const nameOf = (id: number) => shortName(names[String(id)]) || undefined;

  // Problems, shortly after a change.
  useEffect(() => {
    if (!file) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      extraDropsProblems(file).then((found) => { if (!cancelled) setProblems(found); }).catch((problem) => { if (!cancelled) setError(messageOf(problem)); });
    }, 350);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [elementsPath, file]);

  const commit = useCallback((next: DropFile, key: string) => {
    setHistory((current) => {
      if (current.cursor < 0) return current;
      const states = current.states.slice(0, current.cursor + 1);
      const keys = current.keys.slice(0, current.cursor);
      if (current.cursor > 0 && keys[current.cursor - 1] === key) { states[current.cursor] = next; return { states, cursor: current.cursor, keys }; }
      states.push(next);
      keys.push(key);
      if (states.length > 201) { states.shift(); keys.shift(); return { states, cursor: states.length - 1, keys }; }
      return { states, cursor: current.cursor + 1, keys };
    });
    setNote(null);
  }, []);
  const setTable = (index: number, change: Partial<DropTable>, key: string) => {
    if (!file) return;
    commit({ ...file, tables: file.tables.map((table, at) => at === index ? { ...table, ...change } : table) }, `${index}:${key}`);
  };
  const undo = useCallback(() => setHistory((current) => current.cursor > 0 ? { ...current, cursor: current.cursor - 1 } : current), []);
  const redo = useCallback(() => setHistory((current) => current.cursor + 1 < current.states.length ? { ...current, cursor: current.cursor + 1 } : current), []);

  const cloneSelected = useCallback(() => {
    if (!file || selected === null) return;
    const source = file.tables[selected];
    const tables = [...file.tables];
    tables.splice(selected + 1, 0, { ...source, name: `${source.name} (copy)`.slice(0, 60), nameRaw: [], monsters: [] });
    commit({ ...file, tables }, `clone:${Date.now()}`);
    setSelected(selected + 1);
    setNote("Cloned without monsters: a monster may be in one table only (gs does not start otherwise). Add the monsters it is for.");
  }, [commit, file, selected]);
  const deleteSelected = useCallback(() => {
    if (!file || selected === null) return;
    const table = file.tables[selected];
    if (!window.confirm(`Delete “${table.name}” (${count(table.monsters.length)} monsters, ${count(usedItems(table).length)} items)?\n\nIts monsters drop only their own elements.data drops then. Undo brings it back.`)) return;
    commit({ ...file, tables: file.tables.filter((_, index) => index !== selected) }, `delete:${Date.now()}`);
    setSelected(file.tables.length > 1 ? Math.min(selected, file.tables.length - 2) : null);
  }, [commit, file, selected]);

  const saveTo = useCallback(async (target: string, replaceChanged = false) => {
    if (!file || !meta || busy) return;
    const errors = (problems ?? []).filter((problem) => problem.severity === "error");
    if (!replaceChanged && errors.length && !window.confirm(`The server would not start with this file (gs stops with -6):\n\n${errors.slice(0, 6).map((problem) => `• ${problem.table !== null ? `${file.tables[problem.table]?.name}: ` : ""}${problem.message}`).join("\n")}${errors.length > 6 ? "\n…" : ""}\n\nSave anyway?`)) { setProblemsOpen(true); return; }
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const same = target.toLowerCase() === meta.path.toLowerCase();
      const report = await saveExtraDrops(target, same ? meta.token : null, file, backup && !backedUp.current.has(target.toLowerCase()), replaceChanged);
      backedUp.current.add(target.toLowerCase());
      saved.current = file;
      setMeta({ path: report.path, size: report.size, token: report.token });
      setHistory((current) => ({ ...current }));
      try { localStorage.setItem(LAST_KEY, report.path); } catch { /* optional */ }
      setNote(`Saved ${count(file.tables.length)} tables to ${report.path}.${report.backup ? ` Backup: ${report.backup}.` : ""} Restart the game server (gs) to load them.`);
    } catch (problem) {
      const message = messageOf(problem);
      if (message.startsWith("CHANGED_ON_DISK:") && window.confirm(`${message.slice("CHANGED_ON_DISK:".length).trim()}\n\nReplace it with the open tables?`)) {
        setBusy(false);
        await saveTo(target, true);
        return;
      }
      setError(message);
    } finally {
      setBusy(false);
    }
  }, [backup, busy, file, meta, problems]);
  const saveCurrent = useCallback(() => { if (meta && dirty) void saveTo(meta.path); }, [dirty, meta, saveTo]);
  const saveAs = useCallback(async () => {
    if (!meta || busy) return;
    const target = await save({ defaultPath: meta.path, title: "Save extra_drops.sev as", filters: [{ name: "extra_drops.sev", extensions: ["sev"] }] });
    if (typeof target === "string") await saveTo(target);
  }, [busy, meta, saveTo]);

  const compare = useCallback(async () => {
    if (!meta) return;
    const picked = await open({ multiple: false, directory: false, defaultPath: meta.path, title: "Compare with another extra_drops.sev", filters: [{ name: "extra_drops.sev", extensions: ["sev"] }, { name: "All files", extensions: ["*"] }] });
    if (typeof picked !== "string") return;
    try {
      const read = await readExtraDrops(picked);
      setOther({ path: read.path, file: read.file });
    } catch (problem) {
      setError(messageOf(problem));
    }
  }, [meta]);
  const setVersion = useCallback((version: number) => {
    if (!file || file.version === version) return;
    if (version === 1 && file.tables.some((table) => table.extra !== 0) && !window.confirm("Version 1 has no extra value per table: the values are dropped (the zx_18 server source ignores them). Convert?")) return;
    commit({ version, tables: file.tables.map((table) => ({ ...table, extra: version === 2 ? table.extra || 1 : 0 })) }, `version:${Date.now()}`);
    setNote(version === 1 ? "Version 1: every server reads it." : "Version 2: the 2013 server cannot read it (gs stops with -5); the zx_18 server can. New extra values are 1, the most common value in official files.");
  }, [commit, file]);
  const copyTables = (copies: DropTable[]) => {
    if (!file) return;
    const tables = [...file.tables];
    let added = 0, replaced = 0;
    for (const copy of copies) {
      const table = { ...copy, extra: file.version === 2 ? copy.extra || 1 : 0 };
      const at = tables.findIndex((entry) => entry.name === copy.name);
      if (at >= 0) { tables[at] = table; replaced++; } else { tables.push(table); added++; }
    }
    commit({ ...file, tables }, `copy:${Date.now()}`);
    setNote(`Copied from ${other?.path.split(/[\\/]/).pop()}: ${count(replaced)} replaced, ${count(added)} added. Check Problems for monsters now in two tables.`);
  };

  useImperativeHandle(ref, () => ({ choose: () => void choose(), openPath: (path) => guard(() => void load(path)), save: saveCurrent, saveAs: () => void saveAs(), undo, redo, cloneSelected, deleteSelected, openProblems: () => setProblemsOpen(true), compare: () => void compare(), setVersion, showId: (id) => { setQuery(String(id)); const found = file?.tables.findIndex((entry) => entry.monsters.includes(id) || entry.items.some((item) => item.id === id)) ?? -1; if (found >= 0) setSelected(found); } }), [compare, file, setVersion, choose, cloneSelected, deleteSelected, guard, load, redo, saveAs, saveCurrent, undo]);
  useEffect(() => onStateChange({ loaded: !!file, dirty, canUndo: history.cursor > 0, canRedo: history.cursor + 1 < history.states.length, path: meta?.path ?? null, tables: file?.tables.length ?? 0 }), [dirty, file, history.cursor, history.states.length, meta?.path, onStateChange]);

  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      if (picking || other) return;
      const mod = event.ctrlKey || event.metaKey;
      const typing = event.target instanceof HTMLElement && /^(INPUT|TEXTAREA|SELECT)$/.test(event.target.tagName);
      const key = event.key.toLowerCase();
      if (event.key === "Escape" && problemsOpen) { event.preventDefault(); setProblemsOpen(false); return; }
      if (mod && event.shiftKey && key === "m") { event.preventDefault(); event.stopImmediatePropagation(); setProblemsOpen(true); return; }
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
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [active, choose, cloneSelected, deleteSelected, other, picking, problemsOpen, redo, saveAs, saveCurrent, undo]);

  const flagged = useMemo(() => {
    const out = new Map<number, string>();
    for (const problem of problems ?? []) if (problem.table !== null && (problem.severity === "error" || !out.has(problem.table))) out.set(problem.table, problem.severity);
    return out;
  }, [problems]);
  const visible = useMemo(() => {
    const text = query.trim().toLowerCase();
    const number = /^\d+$/.test(text) ? Number(text) : null;
    return (file?.tables ?? []).map((table, index) => ({ table, index })).filter(({ table }) => !text
      || table.name.toLowerCase().includes(text)
      || (number !== null && (table.monsters.includes(number) || table.items.some((item) => item.id === number)))
      || (text.length > 1 && number === null && (table.monsters.some((id) => names[String(id)]?.toLowerCase().includes(text)) || usedItems(table).some((item) => names[String(item.id)]?.toLowerCase().includes(text)))));
  }, [file, names, query]);

  if (!file || !meta) return <section className="path-data-pane empty">
    <div className="drop-card path-data-empty">
      <Dices size={30} />
      <h2>Open extra_drops.sev</h2>
      <p className="muted">The server's extra drop tables (gs.conf <span className="mono">[Template] DropData</span>, in the gs config folder): for listed monsters, how many items drop and which, replacing or adding to their elements.data drops.</p>
      <button className="btn primary" onClick={() => void choose()} disabled={busy}>{busy ? <Loader2 size={14} className="spin" /> : <FolderOpen size={14} />} Choose extra_drops.sev…</button>
      {error && <div className="se-problems" role="alert">{error}</div>}
    </div>
  </section>;

  const table = selected !== null ? file.tables[selected] ?? null : null;
  const errors = (problems ?? []).filter((problem) => problem.severity === "error").length;
  const used = table ? usedItems(table) : [];
  const countSum = table ? f32Sum(table.counts) : 0;
  const itemSum = f32Sum(used.map((item) => item.chance));
  const expected = table ? table.counts.reduce((sum, value, index) => sum + value * index, 0) : 0;
  const owners = new Map<number, number>();
  file.tables.forEach((entry, index) => entry.monsters.forEach((id) => { if (!owners.has(id)) owners.set(id, index); }));

  const setItems = (items: DropTable["items"], key: string) => {
    if (selected === null) return;
    const trimmed = [...items];
    while (trimmed.length && trimmed[trimmed.length - 1].id === 0 && trimmed[trimmed.length - 1].chance === 0) trimmed.pop();
    setTable(selected, { items: trimmed }, key);
  };
  const addMonsters = (ids: number[]) => {
    if (!table || selected === null) return;
    const fresh = ids.filter((id) => id > 0 && !table.monsters.includes(id));
    if (!fresh.length) return;
    const elsewhere = fresh.filter((id) => owners.has(id) && owners.get(id) !== selected);
    setTable(selected, { monsters: [...table.monsters, ...fresh] }, `monsters:${Date.now()}`);
    if (elsewhere.length) setNote(`Monster${elsewhere.length === 1 ? "" : "s"} ${elsewhere.join(", ")} ${elsewhere.length === 1 ? "is" : "are"} also in another table: remove ${elsewhere.length === 1 ? "it" : "them"} there, or gs does not start.`);
  };

  return <section className="path-data-pane drops-pane">
    <header className="path-data-head">
      <div>
        <h2>Extra drops {dirty && <span className="tag warn">unsaved</span>}</h2>
        <div className="path-data-file-line">
          <span className="path-data-location mono muted small truncate" title={meta.path}>{meta.path}</span>
          <span className="path-data-badge"><b>Tables:</b> {count(file.tables.length)}</span>
          <label className="path-data-badge" title="Version 1: every server reads it. Version 2 (zx_18 server): one more value per table; the 2013 server cannot read it."><b>Version:</b> <select className="drops-version" value={file.version} onChange={(event) => setVersion(Number(event.target.value))}><option value={1}>1</option><option value={2}>2</option></select></label>
          <span className="path-data-badge"><b>Size:</b> {bytes(meta.size)}</span>
        </div>
      </div>
      <button className="icon-btn" onClick={undo} disabled={busy || history.cursor <= 0} title="Undo (Ctrl+Z)"><Undo2 size={15} /></button>
      <button className="icon-btn" onClick={redo} disabled={busy || history.cursor + 1 >= history.states.length} title="Redo (Ctrl+Y)"><Redo2 size={15} /></button>
      <button className="btn" onClick={() => setProblemsOpen(true)} title="Problems (Ctrl+Shift+M)">{errors ? <CircleAlert size={13} className="danger-icon" /> : <TriangleAlert size={13} />} Problems{problems ? ` ${count(problems.length)}` : ""}</button>
      <button className="btn primary" onClick={saveCurrent} disabled={busy || !dirty}><Save size={13} /> Save</button>
      <label className="check"><input type="checkbox" checked={backup} onChange={(event) => { setBackup(event.target.checked); try { localStorage.setItem(BACKUP_KEY, event.target.checked ? "1" : "0"); } catch { /* optional */ } }} /> Backup</label>
      <button className="btn small" onClick={() => void saveAs()} disabled={busy}>Save as…</button>
      <button className="btn" onClick={() => void choose()} disabled={busy}><FolderOpen size={13} /> Open…</button>
    </header>
    {(error || note) && <div className={error ? "path-data-message error" : "path-data-message"} role={error ? "alert" : "status"}>
      {error ? <AlertTriangle size={14} /> : <Info size={14} />}{error || note} <button className="link" onClick={() => { setError(null); setNote(null); }}>Dismiss</button>
    </div>}
    {!elementsPath && <div className="path-data-message">Open the server's elements.data to see monster and item names and to check their IDs.</div>}
    <div className="drops-body">
      <aside className="dyn-task-list">
        <div className="dyn-task-search"><Search size={13} /><input value={query} placeholder="Table, monster or item (name or ID)" onChange={(event) => setQuery(event.target.value)} />{query && <button className="icon-btn small" onClick={() => setQuery("")}><X size={12} /></button>}</div>
        <div className="dyn-task-rows" role="listbox">
          {visible.map(({ table: entry, index }) => <button key={index} role="option" aria-selected={index === selected} className={"dyn-task-row drops-row" + (index === selected ? " selected" : "")} onClick={() => setSelected(index)}>
            <span className="mono muted">{index + 1}</span>
            <span className="gshop-row-text"><span className="truncate">{entry.name || <span className="muted">(no name)</span>}</span><span className="muted small">{entry.type === 1 ? "replace" : entry.type === 2 ? "add" : `type ${entry.type}`} · {count(entry.monsters.length)} monster{entry.monsters.length === 1 ? "" : "s"} · {count(usedItems(entry).length)} items</span></span>
            {flagged.get(index) === "error" ? <CircleAlert size={12} className="danger-icon" /> : flagged.has(index) ? <TriangleAlert size={12} className="ai-warn" /> : <span />}
          </button>)}
          {!visible.length && <div className="empty-note">No table matches.</div>}
        </div>
        <footer className="ai-list-foot">
          <span className="muted small">{count(visible.length)} of {count(file.tables.length)}</span>
          <span>
            <button className="icon-btn small" onClick={cloneSelected} disabled={busy || selected === null} title="Clone the table, without monsters (Ctrl+D)"><Copy size={13} /></button>
            <button className="icon-btn small danger" onClick={deleteSelected} disabled={busy || selected === null} title="Delete the table (Del)"><Trash2 size={13} /></button>
          </span>
        </footer>
      </aside>
      <div className="dyn-task-form-scroll drops-form">
        {!table || selected === null ? <div className="empty-note">Select a table.</div> : <>
          {(problems ?? []).filter((problem) => problem.table === selected).map((problem, index) => <div key={index} className={"path-data-message" + (problem.severity === "error" ? " error" : "")}>{problem.severity === "error" ? <CircleAlert size={13} /> : <TriangleAlert size={13} />} {problem.message}</div>)}
          <section className="dyn-section">
            <header><b>Table</b></header>
            <div className="dyn-section-body">
            <div className="drops-grid">
              <label>Name</label><TextInput value={table.name} max={127} onCommit={(name) => setTable(selected, { name }, "name")} />
              <label>Type</label>
              <select className="task-form-select" value={table.type} onChange={(event) => setTable(selected, { type: Number(event.target.value) }, "type")}>
                {Object.entries(TYPES).map(([value, label]) => <option key={value} value={value}>{value} · {label}</option>)}
                {!TYPES[table.type] && <option value={table.type}>{table.type} (not read: gs does not start)</option>}
              </select>
              {file.version === 2 && <><label title="Version 2 only; the zx_18 server source reads it and ignores it">Extra value</label><NumberInput value={table.extra} onCommit={(extra) => setTable(selected, { extra }, "extra")} /></>}
            </div>
            <p className="muted small">{table.type === 1 ? "The monsters drop only from this table (their elements.data drops are skipped)." : table.type === 2 ? "The monsters drop from this table and from their elements.data drops." : ""}</p>
            </div>
          </section>
          <section className="dyn-section">
            <header><b>How many items drop</b><span className={"small " + (exact(countSum) ? "muted" : "ai-warn")}>total {percent(countSum)}</span><span className="muted small">· {Math.round(expected * 100) / 100} items per kill on average</span><span className="spacer" /><button className="btn small" disabled={exact(countSum)} onClick={() => { const next = scaled(table.counts); if (next) setTable(selected, { counts: next }, `counts:${Date.now()}`); }} title="Scale the chances to exactly 100%"><Scale size={12} /> Scale to 100%</button></header>
            <div className="dyn-section-body">
            <div className="drops-counts">
              {table.counts.map((value, index) => <label key={index}><span className="muted small">{index} item{index === 1 ? "" : "s"}</span><PercentInput value={value} invalid={!exact(countSum)} onCommit={(next) => setTable(selected, { counts: table.counts.map((entry, at) => at === index ? next : entry) }, `count:${index}`)} /></label>)}
            </div>
            <p className="muted small">The server first rolls the monster's drop rate (level difference, events), then how many items drop, then each item by its chance. The chances must add up to exactly 100% or gs does not start.</p>
            </div>
          </section>
          <section className="dyn-section">
            <header><b>Monsters</b><span className="muted small">{count(table.monsters.length)}</span><span className="spacer" />
              <button className="btn small" onClick={() => setPicking({ kind: "monster", current: 0, apply: (id) => addMonsters([id]) })}><Search size={12} /> Choose…</button>
              <input className="dyn-input drops-ids" value={monsterIds} placeholder="IDs: 1234, 5678" onChange={(event) => setMonsterIds(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") { addMonsters(monsterIds.split(/[^0-9]+/).filter(Boolean).map(Number)); setMonsterIds(""); } }} />
            </header>
            <div className="dyn-section-body">
            {table.monsters.length ? <table className="dyn-table drops-monsters">
              <thead><tr><th>ID</th><th>Monster</th><th /></tr></thead>
              <tbody>{table.monsters.map((id, index) => {
                const other = owners.get(id);
                const clash = other !== undefined && other !== selected;
                return <tr key={index} className={clash ? "drops-clash" : undefined}>
                  <td className="mono">{id}</td>
                  <td>
                    {nameOf(id) ?? (elementsPath ? <span className="ai-warn small">not in elements.data</span> : <span className="muted small">open elements.data for the name</span>)}
                    {clash && <div className="ai-warn small">Also in “{file.tables[other].name}”: gs does not start</div>}
                  </td>
                  <td><button className="icon-btn small danger" title="Remove from this table" onClick={() => setTable(selected, { monsters: table.monsters.filter((_, at) => at !== index) }, `monsters:${Date.now()}`)}><Trash2 size={12} /></button></td>
                </tr>;
              })}</tbody>
            </table> : <span className="muted small">No monster uses this table.</span>}
            </div>
          </section>
          <section className="dyn-section">
            <header><b>Items</b><span className="muted small">{count(used.length)} of 256</span><span className={"small " + (exact(itemSum) ? "muted" : "ai-warn")}>total {percent(itemSum)}</span><span className="spacer" />
              <button className="btn small" disabled={exact(itemSum) || !used.length} onClick={() => { const next = scaled(used.map((item) => item.chance)); if (next) setItems([...used.map((item, index) => ({ ...item, chance: next[index] })), ...table.items.slice(used.length)], `scale:${Date.now()}`); }} title="Scale the chances of the used items to exactly 100%"><Scale size={12} /> Scale to 100%</button>
              <button className="btn small" disabled={used.length === table.items.filter((item) => item.id !== 0).length} onClick={() => setItems([...table.items.filter((item) => item.id !== 0), ...table.items.filter((item) => item.id === 0)], `compact:${Date.now()}`)} title="Move empty slots to the end (the official export does this): the server stops at the first empty slot"><ArrowDown size={12} /> Empty slots last</button>
              <button className="btn small" disabled={used.length < 2} onClick={() => setItems([...[...used].sort((a, b) => b.chance - a.chance), ...table.items.slice(used.length)], `sort:${Date.now()}`)} title="Sort the used items by chance, highest first"><ArrowDownWideNarrow size={12} /> Sort</button>
            </header>
            <div className="dyn-section-body">
            <table className="dyn-table drops-items">
              <thead><tr><th>#</th><th>Item</th><th>Chance</th><th /></tr></thead>
              <tbody>{table.items.map((item, index) => {
                const ignored = index >= used.length;
                return <tr key={index} className={ignored ? "drops-ignored" : undefined} title={ignored ? "After an empty slot: the server ignores it" : undefined}>
                  <td className="mono muted">{index + 1}</td>
                  <td><span className="dyn-item-cell"><NumberInput value={item.id} onCommit={(id) => setItems(table.items.map((entry, at) => at === index ? { ...entry, id } : entry), `item:${index}`)} /><button className="icon-btn small" title="Choose an item" onClick={() => setPicking({ kind: "item", current: item.id, apply: (id) => setItems(table.items.map((entry, at) => at === index ? { ...entry, id } : entry), `item:${index}`) })}><Search size={12} /></button><span className="truncate drops-name">{item.id ? nameOf(item.id) ?? (elementsPath ? <span className="ai-warn small">not found</span> : null) : <span className="muted small">empty</span>}</span></span></td>
                  <td><PercentInput value={item.chance} onCommit={(chance) => setItems(table.items.map((entry, at) => at === index ? { ...entry, chance } : entry), `chance:${index}`)} /></td>
                  <td className="nowrap">
                    <button className="icon-btn small" disabled={index === 0} title="Move up" onClick={() => { const items = [...table.items]; [items[index - 1], items[index]] = [items[index], items[index - 1]]; setItems(items, `move:${Date.now()}`); }}><ArrowUp size={12} /></button>
                    <button className="icon-btn small" disabled={index + 1 >= table.items.length} title="Move down" onClick={() => { const items = [...table.items]; [items[index + 1], items[index]] = [items[index], items[index + 1]]; setItems(items, `move:${Date.now()}`); }}><ArrowDown size={12} /></button>
                    <button className="icon-btn small danger" title="Remove (later items move up)" onClick={() => setItems(table.items.filter((_, at) => at !== index), `remove:${Date.now()}`)}><Trash2 size={12} /></button>
                  </td>
                </tr>;
              })}</tbody>
            </table>
            <button className="btn small drops-add" disabled={table.items.length >= 256} onClick={() => setPicking({ kind: "item", current: 0, apply: (id) => setItems([...used, { id, chance: 0 }, ...table.items.slice(used.length)], `add:${Date.now()}`) })}><Plus size={12} /> Add an item…</button>
            <p className="muted small">The server keeps the items up to the first empty slot; their chances must add up to exactly 100% or gs does not start. A new item starts at 0%: give it a chance, then scale.</p>
            </div>
          </section>
        </>}
      </div>
    </div>
    {other && <DropsCompare other={other} file={file} nameOf={nameOf} onClose={() => setOther(null)} onChooseOther={() => void compare()} onCopy={copyTables} />}
    {picking && <ValuePicker search={(text, page) => pickEssence(picking.kind, text, page, picking.current || null)} onApply={async (value) => { const id = Number(value); if (!id) return `Choose ${picking.kind === "monster" ? "a monster" : "an item"}`; picking.apply(id); setPicking(null); return null; }} onClose={() => setPicking(null)} />}
    {problemsOpen && <div className="modal-backdrop" onMouseDown={() => setProblemsOpen(false)}>
      <div className="modal npcgen-problems-dialog" role="dialog" aria-label="Problems" onMouseDown={(event) => event.stopPropagation()}>
        <header className="modal-head"><h3>Problems</h3><span className="muted small">{count(errors)} error{errors === 1 ? "" : "s"}</span><span className="spacer" /><button className="icon-btn" onClick={() => setProblemsOpen(false)} aria-label="Close"><X size={16} /></button></header>
        <div className="npcgen-problems-list">
          {!problems ? <div className="empty-note"><Loader2 size={14} className="spin" /> Checking…</div> : !problems.length ? <div className="empty-note">No problems: the server loads every table.</div> : problems.map((problem, index) => <button key={index} className={"npcgen-problem " + problem.severity} onClick={() => { if (problem.table !== null) { setSelected(problem.table); setQuery(""); } setProblemsOpen(false); }}>
            {problem.severity === "error" ? <CircleAlert size={14} /> : problem.severity === "warning" ? <TriangleAlert size={14} /> : <Info size={14} />}
            <span>{problem.table !== null && <b>{file.tables[problem.table]?.name || `Table ${problem.table + 1}`}</b>} {problem.message}</span>
          </button>)}
        </div>
        <footer className="modal-foot muted small">The server's checks when it loads the file (template/extra_drop_table.cpp, global_drop.cpp): any error stops gs (-6).{elementsPath ? " Monsters and items are checked against the open elements.data." : " Open elements.data to check monster and item IDs."}</footer>
      </div>
    </div>}
  </section>;
});
