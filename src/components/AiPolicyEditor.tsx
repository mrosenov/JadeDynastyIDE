import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, Brain, ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight, FolderOpen, Loader2, Search, X } from "lucide-react";
import { aiPolicyPolicy, aiPolicySearch, aiPolicyUsedBy, aiPolicyView, dynTaskLabels, openAiPolicy, skillNames } from "../elements/api";
import { bytes, count } from "../elements/format";
import { formatDuration } from "../elements/time";
import type { AiCondition, AiFile, AiOperation, AiParam, AiPolicyView, AiTrigger } from "../elements/types";

export interface AiPolicyEditorState {
  loaded: boolean;
  path: string | null;
  policies: number;
}

export interface AiPolicyEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
  /** Selects the policy of an ID (the first one, which the server uses), once a file is open. */
  showPolicy: (id: number) => void;
}

interface Props {
  defaultPath?: string | null;
  active: boolean;
  /** elements.data open: names of monsters, items and tasks, and the monsters using each policy. */
  elementsPath: string | null;
  onStateChange: (state: AiPolicyEditorState) => void;
}

const ATTACK_TYPES = ["Melee", "Ranged (physical)", "Magic", "Melee and ranged"];
const PATH_TYPES = ["Stop at the end", "Go back along the path", "Loop"];
/** Channels a talk text can start with (op_say). */
const CHANNELS: Record<string, string> = { $B: "World broadcast", $A: "Broadcast (channel 9)", $F: "Battle faction channel", $T: "Battle channel" };
const CATEGORY_HINTS: Record<string, string> = {
  Heartbeat: "Tested every second (HP, random, aggro, distance, variables)",
  Timer: "Tested when one of its timers fires",
  "Combat start": "When combat starts",
  Kill: "When it kills its target",
  Death: "When it dies",
  "Path end": "When it reaches the end of a path",
  Birth: "When it spawns",
  "Skill hit": "When a skill hits it",
  "Leave combat": "When it leaves combat",
};
/** Policies per page of the list. */
const PAGE = 200;

/** First / previous / a page number to type / next / last. */
function Pager({ page, pages, onPage }: { page: number; pages: number; onPage: (page: number) => void }) {
  const [draft, setDraft] = useState(String(page + 1));
  useEffect(() => setDraft(String(page + 1)), [page]);
  const go = (next: number) => {
    const clamped = Math.max(0, Math.min(pages - 1, next));
    onPage(clamped);
    // A number past the end shows the last page (and the box says so, even when the page stays).
    setDraft(String(clamped + 1));
  };
  const commit = () => {
    const number = Math.trunc(Number(draft));
    if (Number.isFinite(number) && number >= 1) go(number - 1);
    else setDraft(String(page + 1));
  };
  return <span className="ai-pager">
    <button className="icon-btn small" onClick={() => go(0)} disabled={page === 0} title="First page"><ChevronsLeft size={14} /></button>
    <button className="icon-btn small" onClick={() => go(page - 1)} disabled={page === 0} title="Previous page"><ChevronLeft size={14} /></button>
    <span className="small">Page <input className="dyn-input ai-page-input" inputMode="numeric" value={draft} aria-label="Page" onChange={(event) => setDraft(event.target.value.replace(/\D/g, ""))} onBlur={commit} onKeyDown={(event) => { if (event.key === "Enter") commit(); if (event.key === "Escape") setDraft(String(page + 1)); }} onFocus={(event) => event.target.select()} /> of {count(pages)}</span>
    <button className="icon-btn small" onClick={() => go(page + 1)} disabled={page + 1 >= pages} title="Next page"><ChevronRight size={14} /></button>
    <button className="icon-btn small" onClick={() => go(pages - 1)} disabled={page + 1 >= pages} title="Last page"><ChevronsRight size={14} /></button>
  </span>;
}

/** Names the browser looked up, by what an ID refers to. */
interface Names {
  skills: Record<string, string>;
  elements: Record<string, string>;
  tasks: Record<string, string>;
}

function channelOf(text: string): [string | null, string] {
  const prefix = text.slice(0, 2);
  return CHANNELS[prefix] ? [CHANNELS[prefix], text.slice(2)] : [null, text];
}

export const AiPolicyEditor = forwardRef<AiPolicyEditorHandle, Props>(function AiPolicyEditor({ defaultPath, active: _active, elementsPath, onStateChange }, ref) {
  const [file, setFile] = useState<AiFile | null>(null);
  const [usedBy, setUsedBy] = useState<Record<string, [number, string][]>>({});
  const [query, setQuery] = useState("");
  const [matches, setMatches] = useState<number[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [page, setPage] = useState(0);
  const rowsRef = useRef<HTMLDivElement>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [policy, setPolicy] = useState<AiPolicyView | null>(null);
  const [trigger, setTrigger] = useState<number | null>(null);
  const [names, setNames] = useState<Names>({ skills: {}, elements: {}, tasks: {} });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [autoOpened, setAutoOpened] = useState<string | null>(null);
  /** A policy ID to select once the file is open. */
  const [wanted, setWanted] = useState<number | null>(null);

  // Restore an open file after switching workspaces.
  useEffect(() => { aiPolicyView().then((view) => { if (view) setFile(view); }).catch(() => {}); }, []);
  useEffect(() => onStateChange({ loaded: !!file, path: file?.path ?? null, policies: file?.policies.length ?? 0 }), [file, onStateChange]);
  useEffect(() => { if (file) aiPolicyUsedBy().then(setUsedBy).catch(() => setUsedBy({})); }, [elementsPath, file]);

  const load = useCallback(async (path: string) => {
    setBusy(true);
    setError(null);
    try {
      const opened = await openAiPolicy(path);
      setFile(opened);
      setSelected(null);
      setPolicy(null);
      setTrigger(null);
      setQuery("");
      setMatches(null);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  }, []);
  useEffect(() => {
    if (!defaultPath || file || autoOpened === defaultPath) return;
    setAutoOpened(defaultPath);
    void load(defaultPath);
  }, [autoOpened, defaultPath, file, load]);
  const choose = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, defaultPath: file?.path || defaultPath || undefined, title: "Open aipolicy.data", filters: [{ name: "aipolicy.data", extensions: ["data"] }] });
    if (typeof picked === "string") await load(picked);
  }, [defaultPath, file?.path, load]);
  useImperativeHandle(ref, () => ({ choose: () => void choose(), openPath: (path) => void load(path), showPolicy: (id) => setWanted(id) }), [choose, load]);
  useEffect(() => {
    if (wanted === null || !file) return;
    setWanted(null);
    const position = file.policies.findIndex((entry) => entry.id === wanted);
    if (position < 0) {
      setError(`Policy ${wanted} is not in ${file.path.split(/[\\/]/).pop()}: the server gives the monster no policy.`);
      return;
    }
    setQuery("");
    setMatches(null);
    setPage(Math.floor(position / PAGE));
    setSelected(file.policies[position].index);
    window.setTimeout(() => document.querySelector(".ai-policy-row.selected")?.scrollIntoView({ block: "center" }), 50);
  }, [file, wanted]);

  // Search in the backend (IDs, monsters, trigger names, texts, IDs operations name), shortly after typing stops.
  useEffect(() => {
    if (!file) return;
    const text = query.trim();
    if (!text) { setMatches(null); return; }
    setSearching(true);
    const timer = window.setTimeout(() => {
      aiPolicySearch(text).then((found) => { setMatches(found); setPage(0); }).catch(() => setMatches([])).finally(() => setSearching(false));
    }, 250);
    return () => window.clearTimeout(timer);
  }, [file, query]);

  // The selected policy and the names its operations need.
  useEffect(() => {
    if (selected === null) { setPolicy(null); return; }
    let cancelled = false;
    aiPolicyPolicy(selected).then((view) => {
      if (cancelled) return;
      setPolicy(view);
      setTrigger(view.triggers.length ? 0 : null);
      const wanted: Record<string, Set<number>> = { skill: new Set(), element: new Set(), task: new Set() };
      const collect = (params: AiParam[]) => params.forEach((param) => {
        if (typeof param.value !== "number" || !param.value) return;
        if (param.refers === "skill") wanted.skill.add(param.value);
        else if (param.refers === "monster" || param.refers === "item" || param.refers === "mine") wanted.element.add(param.value);
        else if (param.refers === "task") wanted.task.add(param.value);
      });
      const walk = (node: AiCondition | null | undefined) => { if (!node) return; collect(node.params); walk(node.left); walk(node.right); };
      view.triggers.forEach((entry) => { walk(entry.condition); entry.operations.forEach((operation) => collect(operation.params)); });
      if (wanted.skill.size) skillNames([...wanted.skill]).then((found) => setNames((current) => ({ ...current, skills: { ...current.skills, ...found } }))).catch(() => {});
      if (wanted.element.size || wanted.task.size) dynTaskLabels([...wanted.element], [...wanted.task]).then((found) => setNames((current) => ({ ...current, elements: { ...current.elements, ...found.elements }, tasks: { ...current.tasks, ...found.tasks } }))).catch(() => {});
    }).catch((problem) => setError(String(problem).replace(/^Error: /, "")));
    return () => { cancelled = true; };
  }, [selected]);

  const list = useMemo(() => {
    if (!file) return [];
    if (matches === null) return file.policies;
    const byIndex = new Map(file.policies.map((entry) => [entry.index, entry]));
    return matches.flatMap((index) => byIndex.get(index) ?? []);
  }, [file, matches]);
  const pages = Math.max(1, Math.ceil(list.length / PAGE));
  // A shorter list (a new search) keeps the page in range.
  useEffect(() => setPage((current) => Math.min(current, pages - 1)), [pages]);
  const pageRows = list.slice(page * PAGE, (page + 1) * PAGE);
  const showPage = (next: number) => {
    setPage(next);
    rowsRef.current?.scrollTo({ top: 0 });
  };
  const monstersOf = (id: number) => usedBy[String(id)] ?? [];
  const selectedTrigger = policy && trigger !== null ? policy.triggers[trigger] ?? null : null;
  const byId = useMemo(() => new Map((policy?.triggers ?? []).map((entry) => [entry.id, entry])), [policy]);
  // Timers: which triggers start and stop each one.
  const timers = useMemo(() => {
    const out = new Map<number, { starts: number[]; stops: number[]; fires: number[] }>();
    const get = (id: number) => { if (!out.has(id)) out.set(id, { starts: [], stops: [], fires: [] }); return out.get(id)!; };
    for (const entry of policy?.triggers ?? []) {
      for (const operation of entry.operations) {
        const id = operation.params[0]?.value;
        if (typeof id !== "number") continue;
        if (operation.kind === 7) get(id).starts.push(entry.id);
        if (operation.kind === 8) get(id).stops.push(entry.id);
      }
      const walk = (node: AiCondition | null | undefined) => { if (!node) return; if (node.kind === 0 && typeof node.params[0]?.value === "number") get(node.params[0].value as number).fires.push(entry.id); walk(node.left); walk(node.right); };
      walk(entry.condition);
    }
    return out;
  }, [policy]);

  const showTrigger = (id: number) => {
    const found = policy?.triggers.findIndex((entry) => entry.id === id) ?? -1;
    if (found >= 0) setTrigger(found);
  };
  const triggerLink = (id: number) => {
    const target = byId.get(id);
    return <button className="link mono" onClick={() => showTrigger(id)} title={target ? target.name || target.expression : "No trigger with this ID in the policy: the server does nothing"}>{id}{target?.name ? ` · ${target.name}` : target ? "" : " (missing)"}</button>;
  };

  /** A parameter with the name of what it refers to. */
  const paramValue = (param: AiParam): ReactNode => {
    const value = param.value;
    if (typeof value === "boolean") return value ? "yes" : "no";
    if (typeof value === "string") return value || <span className="muted">(empty)</span>;
    switch (param.refers) {
      case "skill": return <>{value} {names.skills[String(value)] && <span className="ai-name">{names.skills[String(value)]}</span>}</>;
      case "monster": case "item": case "mine": return value ? <>{value} {names.elements[String(value)] ? <span className="ai-name">{names.elements[String(value)].split(" › ").pop()}</span> : elementsPath ? <span className="muted small">not in elements.data</span> : null}</> : <span className="muted">0 (none)</span>;
      case "task": return <>{value} {names.tasks[String(value)] && <span className="ai-name">{names.tasks[String(value)]}</span>}</>;
      case "trigger": return triggerLink(value);
      case "timer": {
        const info = timers.get(value);
        return <span title={info ? `Started by ${info.starts.join(", ") || "none"}; stopped by ${info.stops.join(", ") || "none"}; tested by ${info.fires.join(", ") || "none"}` : undefined}>timer {value}{info && !info.starts.length ? <span className="ai-warn"> never started</span> : null}</span>;
      }
      case "attack": return ATTACK_TYPES[value] ?? value;
      case "path_type": return PATH_TYPES[value] ?? value;
      case "global": return <>global[{value}]</>;
      default: return param.label.includes("(s") && value >= 60 ? <>{count(value)} <span className="muted small">{formatDuration(value)}</span></> : count(value);
    }
  };

  const conditionNode = (node: AiCondition | null | undefined, key = "root"): ReactNode => {
    if (!node) return <li key={key} className="muted">(missing)</li>;
    return <li key={key}>
      <span className="ai-cond-name">{node.name}</span>
      {node.params.map((param) => <span key={param.label} className="ai-param"><span className="muted small">{param.label}</span> {param.label === "ratio" || param.label === "chance" ? `${Math.round((param.value as number) * 1000) / 10}%` : paramValue(param)}</span>)}
      {(node.left || node.right) && <ul className="ai-cond-children">{node.left && conditionNode(node.left, `${key}l`)}{node.right && conditionNode(node.right, `${key}r`)}</ul>}
    </li>;
  };

  const operationRow = (operation: AiOperation, index: number, entry: AiTrigger) => {
    const text = operation.params.find((param) => param.label === "text");
    const [channel, body] = typeof text?.value === "string" ? channelOf(text.value) : [null, ""];
    return <li key={index} className="ai-op">
      <span className="ai-op-index mono muted">{index + 1}</span>
      <div className="ai-op-body">
        <div className="ai-op-head"><b>{operation.name}</b>{operation.usesTarget && <span className="tag" title="Target">{operation.targetName}{operation.targetMask !== null ? ` 0x${operation.targetMask.toString(16)}` : ""}</span>}{entry.selectOne && <span className="muted small">one of these runs at random</span>}</div>
        {text ? <div className="ai-talk">{channel && <span className="tag warn">{channel}</span>} {body}</div>
          : operation.params.length > 0 && <div className="ai-op-params">{operation.params.map((param) => <span key={param.label} className="ai-param"><span className="muted small">{param.label}</span> {paramValue(param)}</span>)}</div>}
      </div>
    </li>;
  };

  if (!file) return <section className="path-data-pane empty">
    <div className="drop-card path-data-empty">
      <Brain size={30} />
      <h2>Open aipolicy.data</h2>
      <p className="muted">The monster AI the server runs (<span className="mono">PolicyData</span> in gs.conf): policies of triggers, each a condition and the operations it runs. A monster uses the policy of its <span className="mono">common_strategy</span> (fields with the <i>aipolicy</i> role in the schema editor). The browser is read-only for now.</p>
      <button className="btn primary" onClick={() => void choose()} disabled={busy}>{busy ? <Loader2 size={14} className="spin" /> : <FolderOpen size={14} />} Choose aipolicy.data…</button>
      {defaultPath && <button className="btn" onClick={() => void load(defaultPath)} disabled={busy}>Open the configured client's file</button>}
      {error && <div className="se-problems" role="alert">{error}</div>}
    </div>
  </section>;

  return <section className="dyn-tasks-pane" aria-busy={busy}>
    <header className="tasks-head">
      <div><h2>AI policies <span className="tag">read-only</span></h2><div className="tasks-file-line">
        <span className="mono truncate" title={file.path}>{file.path.split(/[\\/]/).slice(-3).join("/")}</span>
        <span className="path-data-badge"><b>Policies:</b> {count(file.policies.length)}</span>
        <span className="path-data-badge"><b>Triggers:</b> {count(file.triggers)}</span>
        <span className="path-data-badge" title="Trigger versions: 11 is the 2013 server source's, 12 the newer server's (more conditions and operations)"><b>Versions:</b> {file.versions.map(([version, number]) => `${version} (${count(number)})`).join(", ")}</span>
        <span className="path-data-badge"><b>Size:</b> {bytes(file.size)}</span>
      </div></div>
      <button className="btn" onClick={() => void choose()} disabled={busy}><FolderOpen size={14} /> Open…</button>
    </header>
    {error && <div className="path-data-message error"><AlertTriangle size={14} /> {error} <button className="link" onClick={() => setError(null)}>Dismiss</button></div>}
    {!elementsPath && <div className="path-data-message">Open the server's elements.data to see which monsters use each policy and the names of monsters, items and tasks.</div>}
    <div className="ai-body">
      <aside className="dyn-task-list">
        <div className="dyn-task-search"><Search size={13} /><input value={query} placeholder="Policy or monster ID, name, text, skill ID…" onChange={(event) => setQuery(event.target.value)} />{searching ? <Loader2 size={12} className="spin" /> : query && <button className="icon-btn small" onClick={() => setQuery("")}><X size={12} /></button>}</div>
        <div className="dyn-task-rows" role="listbox" ref={rowsRef}>
          {pageRows.map((entry) => {
            const monsters = monstersOf(entry.id);
            return <button key={entry.index} role="option" aria-selected={entry.index === selected} className={"dyn-task-row ai-policy-row" + (entry.index === selected ? " selected" : "")} onClick={() => setSelected(entry.index)}>
              <span className="mono">{entry.id}</span>
              <span className="gshop-row-text"><span className="truncate">{monsters.length ? monsters[0][1] || `monster ${monsters[0][0]}` : <span className="muted">{elementsPath ? "no monster uses it" : `${count(entry.triggers)} triggers`}</span>}{monsters.length > 1 && <span className="muted"> +{monsters.length - 1}</span>}</span><span className="muted small">{count(entry.triggers)} triggers · {count(entry.operations)} operations{entry.talks ? ` · ${count(entry.talks)} texts` : ""}</span></span>
              {entry.shadowed ? <span className="tag warn" title="An earlier policy has this ID; the server uses that one">unused</span> : <span />}
            </button>;
          })}
          {!list.length && <div className="empty-note">No policy matches.</div>}
        </div>
        <footer className="ai-list-foot">
          <span className="muted small">{list.length ? `${count(page * PAGE + 1)}–${count(page * PAGE + pageRows.length)}` : "0"} of {count(list.length)}{list.length !== file.policies.length ? ` (${count(file.policies.length)} in the file)` : ""}</span>
          {pages > 1 && <Pager page={page} pages={pages} onPage={showPage} />}
        </footer>
      </aside>
      <aside className="dyn-task-list ai-triggers">
        {!policy ? <div className="empty-note">Select a policy.</div> : <>
          <div className="ai-policy-head">
            <b>Policy {policy.id}</b>
            {policy.shadowedBy !== null && <span className="tag warn" title="The server keeps the first policy of an ID">the server uses policy #{policy.shadowedBy + 1} instead</span>}
            <div className="muted small ai-used-by">{monstersOf(policy.id).length ? <>Used by {monstersOf(policy.id).slice(0, 6).map(([id, name]) => `${name || "?"} (${id})`).join(", ")}{monstersOf(policy.id).length > 6 ? ` and ${monstersOf(policy.id).length - 6} more` : ""}</> : elementsPath ? "No monster in the open elements.data uses it." : null}</div>
          </div>
          <div className="dyn-task-rows" role="listbox">
            {policy.triggers.map((entry, index) => <button key={index} role="option" aria-selected={index === trigger} className={"dyn-task-row ai-trigger-row" + (index === trigger ? " selected" : "") + (!entry.active && !entry.run ? " ai-off" : "")} onClick={() => setTrigger(index)}>
              <span className="mono muted">{entry.id}</span>
              <span className="gshop-row-text">
                <span className="truncate">{entry.name || <span className="muted">(no name)</span>}</span>
                <span className="muted small truncate" title={entry.expression}>{entry.run ? "called · " : ""}{entry.expression}</span>
              </span>
              <span className={"tag ai-cat" + (entry.run ? " sub" : "")} title={entry.run ? "A sub-trigger: only runs when another trigger runs it" : CATEGORY_HINTS[entry.category]}>{entry.run ? "Sub" : entry.category}</span>
            </button>)}
          </div>
        </>}
      </aside>
      <div className="dyn-task-form-scroll">
        {!selectedTrigger ? <div className="empty-note">{policy ? "Select a trigger." : "Policies are lists of triggers. A trigger runs its operations when its condition holds; which list it is tested in depends on its first condition (birth, combat start, timer, heartbeat, …)."}</div> : <>
          <div className="dyn-task-title"><h3>Trigger {selectedTrigger.id}</h3><span className="muted">{selectedTrigger.name}</span><span className="muted small">version {selectedTrigger.version}</span></div>
          <div className="ai-flags">
            {selectedTrigger.run ? <span className="tag" title="Not tested on its own: another trigger's 'Run a trigger' runs it">Sub-trigger</span> : <span className="tag" title={CATEGORY_HINTS[selectedTrigger.category]}>{selectedTrigger.category}</span>}
            {!selectedTrigger.run && <span className={"tag" + (selectedTrigger.active ? " ok" : " warn")} title="Whether it is enabled when the monster spawns (Enable/Disable a trigger change it)">{selectedTrigger.active ? "Enabled at start" : "Disabled at start"}</span>}
            <span className="tag" title="Run condition">{selectedTrigger.battleOnly ? "In combat only" : "Also out of combat"}</span>
            {selectedTrigger.firesOnce && <span className="tag" title="HP below, aggro count, distance and skill hit disable their trigger after it fires (until the monster resets)">Fires once</span>}
            {selectedTrigger.selectOne && <span className="tag">One random operation</span>}
          </div>
          {selectedTrigger.calledBy.length > 0 && <p className="small">Run, enabled or disabled by {selectedTrigger.calledBy.map((id, index) => <span key={id}>{index ? ", " : ""}{triggerLink(id)}</span>)}</p>}
          {selectedTrigger.run && !selectedTrigger.calledBy.length && <p className="small ai-warn">No trigger of this policy runs it: it never runs (the official editor's cleanup removes such triggers).</p>}
          <h4 className="ai-section">When</h4>
          <p className="ai-expression mono">{selectedTrigger.expression}</p>
          <ul className="ai-cond">{conditionNode(selectedTrigger.condition)}</ul>
          <h4 className="ai-section">Then {selectedTrigger.operations.length > 1 && !selectedTrigger.selectOne && <span className="muted small">in order; "Skip the remaining operations" ends the list</span>}</h4>
          {selectedTrigger.operations.length ? <ol className="ai-ops">{selectedTrigger.operations.map((operation, index) => operationRow(operation, index, selectedTrigger))}</ol> : <p className="muted small">No operations.</p>}
        </>}
      </div>
    </div>
  </section>;
});
