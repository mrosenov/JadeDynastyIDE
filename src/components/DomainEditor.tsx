import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, Castle, CircleAlert, Copy, FolderOpen, Info, Loader2, Plus, Redo2, Save, SaveAll, Search, Trash2, TriangleAlert, Undo2, X } from "lucide-react";
import { domainLandTables, domainProblems, dynTaskLabels, openDomains, saveDomains } from "../elements/api";
import { bytes, count } from "../elements/format";
import { formatMoney } from "../elements/money";
import type { DomainData, DomainEntry, DomainFile, DomainProblem, LandRow } from "../elements/types";
import { NumberInput, TextInput } from "./DynTasksEditor";
import { DomainMap } from "./DomainMap";

export interface DomainEditorState {
  loaded: boolean;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  /** The folder of the open files. */
  path: string | null;
  files: number;
}

export interface DomainEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
  save: () => void;
  saveAll: () => void;
  undo: () => void;
  redo: () => void;
  cloneSelected: () => void;
  deleteSelected: () => void;
  openProblems: () => void;
}

interface Props {
  defaultPath?: string | null;
  active: boolean;
  onStateChange: (state: DomainEditorState) => void;
}

interface History {
  /** Each state: the data of every open file (unchanged files share their object). */
  states: DomainData[][];
  cursor: number;
  keys: string[];
}

const TYPES = ["None", "First class", "Second class", "Third class"];
const DAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const BACKUP_KEY = "jdide.domains.backup";
const I32_MIN = -2147483648, I32_MAX = 2147483647;
const messageOf = (problem: unknown) => String(problem).replace(/^Error: /, "");
const folderOf = (path: string) => path.replace(/[\\/][^\\/]*$/, "");
/** The land table that overrides a file, by file name. */
const landKey = (name: string) => name.toLowerCase() === "domain1.data" ? "domain1.data" : name.toLowerCase() === "domainlandwar.data" ? "domainlandwar.data" : null;

/** domain.data, domain1–4.data and domainlandwar.data together: one tab per file. */
export const DomainEditor = forwardRef<DomainEditorHandle, Props>(function DomainEditor({ defaultPath, active, onStateChange }, ref) {
  const [files, setFiles] = useState<DomainFile[] | null>(null);
  const [history, setHistory] = useState<History>({ states: [], cursor: -1, keys: [] });
  const [tab, setTab] = useState(0);
  const [selected, setSelected] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [backup, setBackup] = useState(() => { try { return localStorage.getItem(BACKUP_KEY) !== "0"; } catch { return true; } });
  const [land, setLand] = useState<Record<string, LandRow[]>>({});
  const [itemNames, setItemNames] = useState<Record<string, string>>({});
  const [problems, setProblems] = useState<DomainProblem[] | null>(null);
  const [problemsOpen, setProblemsOpen] = useState(false);
  const saved = useRef<DomainData[]>([]);
  const autoOpened = useRef<string | null>(null);
  const backedUp = useRef(new Set<string>());

  const datas = history.cursor >= 0 ? history.states[history.cursor] : [];
  const dirtyFiles = datas.map((data, index) => data !== saved.current[index]);
  const dirty = dirtyFiles.some(Boolean);
  const file = files?.[tab] ?? null;
  const data = datas[tab] ?? null;
  const landRows = file ? land[landKey(file.name) ?? ""] ?? null : null;

  const load = useCallback(async (path: string) => {
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const opened = await openDomains(path);
      saved.current = opened.map((entry) => entry.data);
      setFiles(opened);
      setHistory({ states: [opened.map((entry) => entry.data)], cursor: 0, keys: [] });
      const wanted = opened.findIndex((entry) => entry.path.toLowerCase() === path.toLowerCase());
      setTab(wanted >= 0 ? wanted : 0);
      setSelected(null);
      setQuery("");
    } catch (problem) {
      setError(messageOf(problem));
    } finally {
      setBusy(false);
    }
  }, []);
  useEffect(() => {
    if (!defaultPath || files || autoOpened.current === defaultPath) return;
    autoOpened.current = defaultPath;
    void load(defaultPath);
  }, [defaultPath, files, load]);
  useEffect(() => { domainLandTables().then(setLand).catch(() => setLand({})); }, []);
  // Names of the declaration items land.txt names.
  useEffect(() => {
    const ids = [...new Set(Object.values(land).flat().map((row) => row.item).filter(Boolean))];
    if (ids.length) dynTaskLabels(ids, []).then((found) => setItemNames(found.elements)).catch(() => {});
  }, [land]);
  const guard = useCallback((action: () => void) => {
    if (!dirty || window.confirm("Discard the unsaved domain file changes?")) action();
  }, [dirty]);
  const choose = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, defaultPath: files?.[0]?.path || defaultPath || undefined, title: "Open a domain file (all domain*.data files of its folder open)", filters: [{ name: "domain*.data", extensions: ["data"] }] });
    if (typeof picked === "string") guard(() => void load(picked));
  }, [defaultPath, files, guard, load]);

  // Problems of the shown file, shortly after it changes.
  useEffect(() => {
    if (!file || !data) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      domainProblems(file.name, data, landRows).then((found) => { if (!cancelled) setProblems(found); }).catch((problem) => { if (!cancelled) setError(messageOf(problem)); });
    }, 300);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [data, file, landRows]);

  const commit = useCallback((next: DomainData, key: string) => {
    setHistory((current) => {
      if (current.cursor < 0) return current;
      const state = current.states[current.cursor].map((entry, index) => index === tab ? next : entry);
      const states = current.states.slice(0, current.cursor + 1);
      const keys = current.keys.slice(0, current.cursor);
      if (current.cursor > 0 && keys[current.cursor - 1] === key) {
        states[current.cursor] = state;
        return { states, cursor: current.cursor, keys };
      }
      states.push(state);
      keys.push(key);
      if (states.length > 201) { states.shift(); keys.shift(); return { states, cursor: states.length - 1, keys }; }
      return { states, cursor: current.cursor + 1, keys };
    });
    setNote(null);
  }, [tab]);
  const setDomain = (index: number, change: Partial<DomainEntry>, key: string) => {
    if (!data) return;
    commit({ ...data, domains: data.domains.map((domain, at) => at === index ? { ...domain, ...change } : domain) }, `${tab}:${index}:${key}`);
  };
  const undo = useCallback(() => setHistory((current) => current.cursor > 0 ? { ...current, cursor: current.cursor - 1 } : current), []);
  const redo = useCallback(() => setHistory((current) => current.cursor + 1 < current.states.length ? { ...current, cursor: current.cursor + 1 } : current), []);

  const cloneSelected = useCallback(() => {
    if (!data || selected === null) return;
    const source = data.domains[selected];
    const id = data.domains.reduce((max, domain) => Math.max(max, domain.id), 0) + 1;
    const domains = [...data.domains];
    domains.splice(selected + 1, 0, { ...source, id, nameRaw: [] });
    commit({ ...data, domains }, `clone:${Date.now()}`);
    setSelected(selected + 1);
    setNote(`Cloned ${source.name || source.id} as ID ${id}. Its outline is the same as the original's.`);
  }, [commit, data, selected]);
  const deleteSelected = useCallback(() => {
    if (!data || selected === null) return;
    const target = data.domains[selected];
    const pointing = data.domains.filter((domain, index) => index !== selected && domain.neighbours.includes(target.id));
    if (!window.confirm(`Delete ${target.name || "the domain"} (ID ${target.id})?${pointing.length ? `\n\nIt is removed from the neighbours of ${pointing.map((domain) => domain.name || domain.id).join(", ")}.` : ""}${file?.name.toLowerCase() === "domain.data" ? "\n\nThe map editor's precincts name domain.data IDs as their city." : ""}\n\nUndo brings it back.`)) return;
    const duplicates = data.domains.filter((domain) => domain.id === target.id).length > 1;
    const domains = data.domains
      .filter((_, index) => index !== selected)
      .map((domain) => duplicates || !domain.neighbours.includes(target.id) ? domain : { ...domain, neighbours: domain.neighbours.filter((id) => id !== target.id) });
    commit({ ...data, domains }, `delete:${Date.now()}`);
    setSelected(null);
  }, [commit, data, file, selected]);

  const saveFiles = useCallback(async (indexes: number[], replaceChanged = false) => {
    if (!files || busy || !indexes.length) return;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      if (!replaceChanged) {
        for (const index of indexes) {
          const found = await domainProblems(files[index].name, datas[index], land[landKey(files[index].name) ?? ""] ?? null);
          const errors = found.filter((problem) => problem.severity === "error");
          if (errors.length && !window.confirm(`${files[index].name} has ${count(errors.length)} error${errors.length === 1 ? "" : "s"} the client may misread:\n\n${errors.slice(0, 6).map((problem) => `• ${problem.message}`).join("\n")}\n\nSave anyway?`)) return;
        }
      }
      const targets = indexes.map((index) => ({ path: files[index].path, token: files[index].token, data: datas[index] }));
      const report = await saveDomains(targets, backup && indexes.some((index) => !backedUp.current.has(files[index].path.toLowerCase())), replaceChanged);
      for (const index of indexes) backedUp.current.add(files[index].path.toLowerCase());
      const byPath = new Map(report.files.map(([path, size, token]) => [path.toLowerCase(), { size, token }]));
      const nextFiles = files.map((entry, index) => {
        const written = byPath.get(entry.path.toLowerCase());
        return written && indexes.includes(index) ? { ...entry, size: written.size, token: written.token, data: datas[index] } : entry;
      });
      saved.current = saved.current.map((entry, index) => indexes.includes(index) ? datas[index] : entry);
      setFiles(nextFiles);
      setNote(`Saved ${indexes.map((index) => files[index].name).join(", ")}.${report.backup ? ` Backup: ${report.backup}.` : ""} Only the client reads these files: every player needs them.`);
    } catch (problem) {
      const message = messageOf(problem);
      if (message.startsWith("CHANGED_ON_DISK:") && window.confirm(`${message.slice("CHANGED_ON_DISK:".length).trim()}\n\nReplace it with the open data?`)) {
        setBusy(false);
        await saveFiles(indexes, true);
        return;
      }
      setError(message);
    } finally {
      setBusy(false);
    }
  }, [backup, busy, datas, files, land]);
  const saveCurrent = useCallback(() => { if (dirtyFiles[tab]) void saveFiles([tab]); }, [dirtyFiles, saveFiles, tab]);
  const saveAll = useCallback(() => void saveFiles(dirtyFiles.map((flag, index) => flag ? index : -1).filter((index) => index >= 0)), [dirtyFiles, saveFiles]);

  useImperativeHandle(ref, () => ({ choose: () => void choose(), openPath: (path) => guard(() => void load(path)), save: saveCurrent, saveAll, undo, redo, cloneSelected, deleteSelected, openProblems: () => setProblemsOpen(true) }), [choose, cloneSelected, deleteSelected, guard, load, redo, saveAll, saveCurrent, undo]);
  useEffect(() => onStateChange({ loaded: !!files, dirty, canUndo: history.cursor > 0, canRedo: history.cursor + 1 < history.states.length, path: files?.[0] ? folderOf(files[0].path) : null, files: files?.length ?? 0 }), [dirty, files, history.cursor, history.states.length, onStateChange]);

  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
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
        else if (key === "s") event.shiftKey ? saveAll() : saveCurrent();
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
  }, [active, choose, cloneSelected, deleteSelected, problemsOpen, redo, saveAll, saveCurrent, undo]);

  const flagged = useMemo(() => new Set((problems ?? []).filter((problem) => problem.severity !== "note" && problem.domain !== null).map((problem) => problem.domain!)), [problems]);
  const effective = useMemo(() => landRows ? new Map(landRows.map((row) => [row.id, { name: row.name, type: row.level >= 1 && row.level <= 3 ? row.level : 0 }])) : undefined, [landRows]);
  const visible = useMemo(() => {
    const text = query.trim().toLowerCase();
    return (data?.domains ?? []).map((domain, index) => ({ domain, index })).filter(({ domain }) => !text || String(domain.id) === text || domain.name.toLowerCase().includes(text));
  }, [data, query]);

  if (!files || !data || !file) return <section className="path-data-pane empty">
    <div className="drop-card path-data-empty">
      <Castle size={30} />
      <h2>Open the domain files</h2>
      <p className="muted">The client's territory maps: domain.data (guild map), domain1.data (territory war), domain2–4.data (world map layers) and domainlandwar.data (land war). Choose one; every domain*.data file of its folder opens, one tab each.</p>
      <button className="btn primary" onClick={() => void choose()} disabled={busy}>{busy ? <Loader2 size={14} className="spin" /> : <FolderOpen size={14} />} Choose a domain file…</button>
      {defaultPath && <button className="btn" onClick={() => void load(defaultPath)} disabled={busy}>Open the configured client's files</button>}
      {error && <div className="se-problems" role="alert">{error}</div>}
    </div>
  </section>;

  const domain = selected !== null ? data.domains[selected] ?? null : null;
  const landRow = domain && landRows ? landRows.find((row) => row.id === domain.id) ?? null : null;
  const landName = file.name.toLowerCase() === "domainlandwar.data" ? "landwar.txt" : "land.txt";
  const overridden = (field: string) => landRow ? <span className="tag warn" title={`The client takes the ${field} from configs\\${landName}`}>{landName}</span> : null;
  const byId = new Map(data.domains.map((entry) => [entry.id, entry]));
  const errors = (problems ?? []).filter((problem) => problem.severity === "error").length;
  const times = data.times;
  const setTimes = (next: [number, number, number][] | null, key: string) => commit({ ...data, times: next }, `${tab}:times:${key}`);

  return <section className="path-data-pane domain-pane">
    <header className="path-data-head">
      <div>
        <h2>Domains {dirty && <span className="tag warn">unsaved</span>}</h2>
        <div className="path-data-file-line">
          <span className="path-data-location mono muted small truncate" title={file.path}>{folderOf(file.path)}</span>
          <span className="path-data-badge"><b>{file.name}:</b> {count(data.domains.length)} domains</span>
          <span className="path-data-badge"><b>Size:</b> {bytes(file.size)}</span>
          {data.magic && <span className="path-data-badge" title="The file starts with the MOXB marker of newer clients (kept when saving)">MOXB</span>}
        </div>
      </div>
      <button className="icon-btn" onClick={undo} disabled={busy || history.cursor <= 0} title="Undo (Ctrl+Z)"><Undo2 size={15} /></button>
      <button className="icon-btn" onClick={redo} disabled={busy || history.cursor + 1 >= history.states.length} title="Redo (Ctrl+Y)"><Redo2 size={15} /></button>
      <button className="btn" onClick={() => setProblemsOpen(true)} title="Problems of this file (Ctrl+Shift+M)">{errors ? <CircleAlert size={13} className="danger-icon" /> : <TriangleAlert size={13} />} Problems{problems ? ` ${count(problems.length)}` : ""}</button>
      <button className="btn primary" onClick={saveCurrent} disabled={busy || !dirtyFiles[tab]} title="Save this file (Ctrl+S)"><Save size={13} /> Save</button>
      <button className="btn" onClick={saveAll} disabled={busy || !dirty} title="Save every changed file (Ctrl+Shift+S)"><SaveAll size={13} /> Save all</button>
      <label className="check"><input type="checkbox" checked={backup} onChange={(event) => { setBackup(event.target.checked); try { localStorage.setItem(BACKUP_KEY, event.target.checked ? "1" : "0"); } catch { /* optional */ } }} /> Backup</label>
      <button className="btn" onClick={() => void choose()} disabled={busy}><FolderOpen size={13} /> Open…</button>
    </header>
    <nav className="domain-tabs" role="tablist">
      {files.map((entry, index) => <button key={entry.path} role="tab" aria-selected={index === tab} className={"domain-tab" + (index === tab ? " active" : "")} onClick={() => { setTab(index); setSelected(null); setProblems(null); }} title={entry.path}>
        <b>{entry.name}</b><span className="muted small">{entry.label}</span>{dirtyFiles[index] && <span className="changed-dot" title="Unsaved changes" />}
      </button>)}
    </nav>
    {(error || note) && <div className={error ? "path-data-message error" : "path-data-message"} role={error ? "alert" : "status"}>
      {error ? <AlertTriangle size={14} /> : <Info size={14} />}{error || note} <button className="link" onClick={() => { setError(null); setNote(null); }}>Dismiss</button>
    </div>}
    {landRows && <div className="path-data-message"><Info size={14} /> The client takes this file's names, classes, declaration items and neighbours from configs\{landName} ({count(landRows.length)} rows){file.name.toLowerCase() === "domainlandwar.data" ? " (by its name; the source of this client version is not available)" : ""}: those fields here only matter for domains it lacks.</div>}
    <div className="domain-body">
      <aside className="dyn-task-list">
        <div className="dyn-task-search"><Search size={13} /><input value={query} placeholder="Name or ID" onChange={(event) => setQuery(event.target.value)} />{query && <button className="icon-btn small" onClick={() => setQuery("")}><X size={12} /></button>}</div>
        <div className="dyn-task-rows" role="listbox">
          {visible.map(({ domain: entry, index }) => {
            const effective = landRows?.find((row) => row.id === entry.id);
            return <button key={index} role="option" aria-selected={index === selected} className={"dyn-task-row domain-row" + (index === selected ? " selected" : "")} onClick={() => setSelected(index)}>
              <span className="mono">{entry.id}</span>
              <span className="gshop-row-text"><span className="truncate">{effective?.name ?? (entry.name || <span className="muted">(no name)</span>)}</span><span className="muted small">{TYPES[effective ? (effective.level >= 1 && effective.level <= 3 ? effective.level : 0) : entry.type] ?? `type ${entry.type}`} · {count(entry.neighbours.filter(Boolean).length)} neighbours</span></span>
              {flagged.has(index) ? <TriangleAlert size={12} className="ai-warn" /> : <span />}
            </button>;
          })}
        </div>
        <footer className="ai-list-foot">
          <span className="muted small">{count(visible.length)} of {count(data.domains.length)}</span>
          <span>
            <button className="icon-btn small" onClick={cloneSelected} disabled={busy || selected === null} title="Clone the domain with the next free ID (Ctrl+D)"><Copy size={13} /></button>
            <button className="icon-btn small danger" onClick={deleteSelected} disabled={busy || selected === null} title="Delete the domain (Del)"><Trash2 size={13} /></button>
          </span>
        </footer>
      </aside>
      <div className="domain-map-wrap">
        <DomainMap domains={data.domains} selected={selected} flagged={flagged} effective={effective} onSelect={setSelected} onMoveCenter={(index, center) => setDomain(index, { center }, `center:${Date.now()}`)} />
        <div className="npcgen-legend domain-legend">{TYPES.map((label, index) => <span key={label}><i className={`domain-swatch type-${index}`} />{label}</span>)}<span className="muted">Wheel: zoom · drag: move · double-click: fit · drag the selected center to move it</span></div>
      </div>
      <aside className="domain-form">
        {domain && selected !== null ? <>
          <h3>{domain.name || "(no name)"} <span className="muted small">ID {domain.id}</span></h3>
          <div className="domain-fields">
            <label>Name {overridden("name")}</label><TextInput value={domain.name} max={15} onCommit={(name) => setDomain(selected, { name }, "name")} />
            <label>ID</label><NumberInput value={domain.id} min={I32_MIN} max={I32_MAX} onCommit={(id) => {
              const pointing = data.domains.filter((entry) => entry.neighbours.includes(domain.id)).length;
              const unique = data.domains.filter((entry) => entry.id === domain.id).length === 1;
              commit({ ...data, domains: data.domains.map((entry, at) => at === selected ? { ...entry, id } : unique ? { ...entry, neighbours: entry.neighbours.map((neighbour) => neighbour === domain.id ? id : neighbour) } : entry) }, `${tab}:${selected}:id`);
              if (pointing && unique) setNote(`Changed the ID in ${count(pointing)} neighbour list${pointing === 1 ? "" : "s"} too.${file.name.toLowerCase() === "domain.data" ? " The map editor's precincts name domain.data IDs as their city." : ""}`);
            }} />
            <label>Class {overridden("class")}</label>
            <select className="task-form-select" value={domain.type} onChange={(event) => setDomain(selected, { type: Number(event.target.value) }, "type")}>
              {TYPES.map((label, index) => <option key={index} value={index}>{index} · {label}</option>)}
              {(domain.type < 0 || domain.type > 3) && <option value={domain.type}>{domain.type} (unknown)</option>}
            </select>
            <label>Reward per week</label><span className="domain-inline"><NumberInput value={domain.reward} min={I32_MIN} max={I32_MAX} onCommit={(reward) => setDomain(selected, { reward }, "reward")} /><span className="muted small">{formatMoney(domain.reward)}</span></span>
            <label>Center</label><span className="domain-inline"><NumberInput value={domain.center[0]} min={I32_MIN} max={I32_MAX} onCommit={(x) => setDomain(selected, { center: [x, domain.center[1]] }, "center")} /><NumberInput value={domain.center[1]} min={I32_MIN} max={I32_MAX} onCommit={(y) => setDomain(selected, { center: [domain.center[0], y] }, "center")} /></span>
            <label>Outline</label><span className="muted small">{count(domain.verts.length)} points, {count(domain.faces.length)} triangles (map image pixels; not editable yet)</span>
            <label>Neighbours {overridden("neighbours")}</label>
            <span className="domain-neighbours">
              {domain.neighbours.map((id, index) => <span key={index} className="domain-chip" title={byId.get(id)?.name ?? (id === 0 ? "empty entry" : "not in this file")}>
                <button className="link" onClick={() => { const at = data.domains.findIndex((entry) => entry.id === id); if (at >= 0) setSelected(at); }}>{byId.get(id)?.name || id}</button>
                <button className="icon-btn small" title="Remove" onClick={() => setDomain(selected, { neighbours: domain.neighbours.filter((_, at) => at !== index) }, `neighbours:${Date.now()}`)}><X size={10} /></button>
              </span>)}
              <select className="task-form-select domain-add" value="" onChange={(event) => { const id = Number(event.target.value); if (event.target.value !== "") setDomain(selected, { neighbours: [...domain.neighbours, id] }, `neighbours:${Date.now()}`); }}>
                <option value="">Add…</option>
                {data.domains.filter((entry) => entry.id !== domain.id && !domain.neighbours.includes(entry.id)).map((entry) => <option key={entry.id} value={entry.id}>{entry.id} · {entry.name || "(no name)"}{entry.neighbours.includes(domain.id) ? " (lists this one)" : ""}</option>)}
              </select>
            </span>
          </div>
          {landRow && <div className="domain-land">
            <h4>What the client uses (configs\{landName})</h4>
            <dl>
              <dt>Name</dt><dd>{landRow.name}</dd>
              <dt>Class</dt><dd>{TYPES[landRow.level >= 1 && landRow.level <= 3 ? landRow.level : 0]} ({landRow.level})</dd>
              <dt>Declaration item</dt><dd><span className="mono">{landRow.item}</span> {itemNames[String(landRow.item)] && <span className="ai-name">{itemNames[String(landRow.item)].split(" › ").pop()}</span>}</dd>
              <dt>Items</dt><dd>{count(landRow.lower)} – {count(landRow.upper)}</dd>
              <dt>Neighbours</dt><dd>{landRow.neighbours.map((id) => byId.get(id)?.name ? `${byId.get(id)!.name} (${id})` : id).join(", ") || "none"}</dd>
            </dl>
          </div>}
          {(problems ?? []).filter((problem) => problem.domain === selected).map((problem, index) => <div key={index} className={"path-data-message" + (problem.severity === "error" ? " error" : "")}><TriangleAlert size={13} /> {problem.message}</div>)}
        </> : <p className="muted small">Select a domain in the list or on the map.</p>}
        <div className="domain-times">
          <h4>Battle times</h4>
          {!times ? <p className="muted small">This file has no battle time list.</p> : <>
            <table className="dyn-table">
              <thead><tr><th>Day</th><th>Hour</th><th>Minute</th><th /></tr></thead>
              <tbody>{times.map((time, index) => <tr key={index}>
                <td><select className="task-form-select" value={time[0]} onChange={(event) => setTimes(times.map((entry, at) => at === index ? [Number(event.target.value), entry[1], entry[2]] : entry), `day:${index}`)}>{DAYS.map((day, value) => <option key={value} value={value}>{day}</option>)}{(time[0] < 0 || time[0] > 6) && <option value={time[0]}>{time[0]}</option>}</select></td>
                <td><NumberInput value={time[1]} max={23} onCommit={(hour) => setTimes(times.map((entry, at) => at === index ? [entry[0], hour, entry[2]] : entry), `hour:${index}`)} /></td>
                <td><NumberInput value={time[2]} max={59} onCommit={(minute) => setTimes(times.map((entry, at) => at === index ? [entry[0], entry[1], minute] : entry), `minute:${index}`)} /></td>
                <td><button className="icon-btn small danger" title="Remove" onClick={() => setTimes(times.filter((_, at) => at !== index), `remove:${Date.now()}`)}><Trash2 size={12} /></button></td>
              </tr>)}</tbody>
            </table>
            <div className="domain-inline">
              <button className="btn small" onClick={() => setTimes([...times, [5, 20, 0]], `add:${Date.now()}`)}><Plus size={12} /> Add a time</button>
              <label className="small">Battles per slot</label><NumberInput value={data.timeMax} min={0} max={I32_MAX} onCommit={(timeMax) => commit({ ...data, timeMax }, `${tab}:timemax`)} />
            </div>
            <p className="muted small">Days count from Monday 00:00. The server reads the times from domain.sev (exported from domain1.data); the first slot always holds 8 battles.</p>
          </>}
        </div>
      </aside>
    </div>
    {problemsOpen && <div className="modal-backdrop" onMouseDown={() => setProblemsOpen(false)}>
      <div className="modal npcgen-problems-dialog" role="dialog" aria-label="Problems" onMouseDown={(event) => event.stopPropagation()}>
        <header className="modal-head"><h3>Problems: {file.name}</h3><span className="spacer" /><button className="icon-btn" onClick={() => setProblemsOpen(false)} aria-label="Close"><X size={16} /></button></header>
        <div className="npcgen-problems-list">
          {!problems ? <div className="empty-note"><Loader2 size={14} className="spin" /> Checking…</div> : !problems.length ? <div className="empty-note">No problems.</div> : problems.map((problem, index) => <button key={index} className={"npcgen-problem " + problem.severity} onClick={() => { if (problem.domain !== null) { setSelected(problem.domain); setQuery(""); } setProblemsOpen(false); }}>
            {problem.severity === "error" ? <CircleAlert size={14} /> : problem.severity === "warning" ? <TriangleAlert size={14} /> : <Info size={14} />}
            <span>{problem.domain !== null && <b>{data.domains[problem.domain]?.name || `Domain ${data.domains[problem.domain]?.id}`}</b>} {problem.message}</span>
          </button>)}
        </div>
        <footer className="modal-foot muted small">From the client's reader and map windows (globaldataman.cpp, DlgTerrWarMap.cpp). Errors can make the client misread the file or crash; notes are probably unintended.</footer>
      </div>
    </div>}
  </section>;
});
