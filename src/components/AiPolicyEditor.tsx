import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState, type ReactNode } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, ArrowDown, ArrowUp, Brain, Eraser, Network, Play, ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight, CircleAlert, Copy, FolderOpen, Info, Loader2, Redo2, Save, Search, Trash2, TriangleAlert, Undo2, X } from "lucide-react";
import { aiPolicyCatalog, aiPolicyCleanup, aiPolicyPolicy, aiPolicyPolicyAction, aiPolicyProblems, aiPolicyRedo, aiPolicySave, aiPolicySearch, aiPolicySetTrigger, aiPolicyTriggerAction, aiPolicyUndo, aiPolicyUnusedSubtriggers, aiPolicyUsedBy, aiPolicyView, dynTaskLabels, openAiPolicy, pickEssence, pickSkill, skillNames, type AiListAction } from "../elements/api";
import { bytes, count } from "../elements/format";
import { formatDuration } from "../elements/time";
import type { AiCatalog, AiCondition, AiEditResult, AiFile, AiOperation, AiParam, AiPolicyView, AiProblem, AiTriggerEdit, PickerResult } from "../elements/types";
import { AiTriggerForm, ATTACK_TYPES, PATH_TYPES, type FormContext } from "./AiTriggerForm";
import { AiPolicyFlow } from "./AiPolicyFlow";
import { AiSimulator } from "./AiSimulator";
import { ValuePicker } from "./ValuePicker";

export interface AiPolicyEditorState {
  loaded: boolean;
  path: string | null;
  policies: number;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
}

export interface AiPolicyEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
  /** Selects the policy of an ID (the first one, which the server uses), once a file is open. */
  showPolicy: (id: number) => void;
  save: () => void;
  saveAs: () => void;
  undo: () => void;
  redo: () => void;
  triggerAction: (action: AiListAction) => void;
  policyAction: (action: "clone" | "delete") => void;
  openProblems: () => void;
  openSimulator: () => void;
  openFlow: () => void;
  /** Removes sub-triggers nothing runs from the shown policy or the whole file. */
  cleanup: (whole: boolean) => void;
}

interface Props {
  defaultPath?: string | null;
  active: boolean;
  /** elements.data open: names of monsters, items and tasks, and the monsters using each policy. */
  elementsPath: string | null;
  onStateChange: (state: AiPolicyEditorState) => void;
}

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
const BACKUP_KEY = "jdide.aiPolicy.backup";
const messageOf = (problem: unknown) => String(problem).replace(/^Error: /, "");

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

export const AiPolicyEditor = forwardRef<AiPolicyEditorHandle, Props>(function AiPolicyEditor({ defaultPath, active, elementsPath, onStateChange }, ref) {
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
  /** The trigger to select when the next policy arrives (from the problems list). */
  const wantedTrigger = useRef<number | null>(null);
  /** A policy an edit already returned: the selection effect need not fetch it. */
  const delivered = useRef<number | null>(null);
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  const [catalog, setCatalog] = useState<AiCatalog | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [backup, setBackup] = useState(() => { try { return localStorage.getItem(BACKUP_KEY) !== "0"; } catch { return true; } });
  const [problems, setProblems] = useState<AiProblem[] | null>(null);
  const [problemsOpen, setProblemsOpen] = useState(false);
  const [simOpen, setSimOpen] = useState(false);
  const [flowOpen, setFlowOpen] = useState(false);
  const [picking, setPicking] = useState<{ refers: string; current: number; apply: (value: number) => void } | null>(null);
  const dirty = !!file?.state.dirty;

  // Restore an open file after switching workspaces.
  useEffect(() => { aiPolicyView().then((view) => { if (view) setFile(view); }).catch(() => {}); }, []);
  useEffect(() => onStateChange({ loaded: !!file, path: file?.path ?? null, policies: file?.policies.length ?? 0, dirty, canUndo: !!file?.state.canUndo, canRedo: !!file?.state.canRedo }), [dirty, file, onStateChange]);
  // The types this file's server reads.
  const filePath = file?.path;
  useEffect(() => { if (filePath) aiPolicyCatalog().then(setCatalog).catch(() => setCatalog(null)); }, [filePath]);
  useEffect(() => { if (file) aiPolicyUsedBy().then(setUsedBy).catch(() => setUsedBy({})); }, [elementsPath, file]);

  const load = useCallback(async (path: string) => {
    if (dirty && !window.confirm("aipolicy.data has unsaved changes. Open another file and discard them?")) return;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const opened = await openAiPolicy(path);
      setFile(opened);
      setCatalog(await aiPolicyCatalog());
      setProblems(null);
      setSelected(null);
      setPolicy(null);
      setTrigger(null);
      setQuery("");
      setMatches(null);
    } catch (problem) {
      setError(messageOf(problem));
    } finally {
      setBusy(false);
    }
  }, [dirty]);
  useEffect(() => {
    if (!defaultPath || file || autoOpened === defaultPath) return;
    setAutoOpened(defaultPath);
    void load(defaultPath);
  }, [autoOpened, defaultPath, file, load]);
  const choose = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, defaultPath: file?.path || defaultPath || undefined, title: "Open aipolicy.data", filters: [{ name: "aipolicy.data", extensions: ["data"] }] });
    if (typeof picked === "string") await load(picked);
  }, [defaultPath, file?.path, load]);
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
    if (delivered.current === selected) { delivered.current = null; return; }
    let cancelled = false;
    aiPolicyPolicy(selected).then((view) => {
      if (cancelled) return;
      setPolicy(view);
      const at = wantedTrigger.current;
      wantedTrigger.current = null;
      setTrigger(at !== null && at < view.triggers.length ? at : view.triggers.length ? 0 : null);
    }).catch((problem) => setError(messageOf(problem)));
    return () => { cancelled = true; };
  }, [selected]);

  // Names of the skills, monsters, items and tasks the shown policy names (those not looked up yet).
  const namesRef = useRef(names);
  namesRef.current = names;
  useEffect(() => {
    if (!policy) return;
    const view = policy;
    {
      const known = namesRef.current;
      const wanted: Record<string, Set<number>> = { skill: new Set(), element: new Set(), task: new Set() };
      const collect = (params: AiParam[]) => params.forEach((param) => {
        if (typeof param.value !== "number" || !param.value) return;
        const key = String(param.value);
        if (param.refers === "skill" && !(key in known.skills)) wanted.skill.add(param.value);
        else if ((param.refers === "monster" || param.refers === "item" || param.refers === "mine") && !(key in known.elements)) wanted.element.add(param.value);
        else if (param.refers === "task" && !(key in known.tasks)) wanted.task.add(param.value);
      });
      const walk = (node: AiCondition | null | undefined) => { if (!node) return; collect(node.params); walk(node.left); walk(node.right); };
      view.triggers.forEach((entry) => { walk(entry.condition); entry.operations.forEach((operation) => collect(operation.params)); });
      const misses = (ids: Set<number>) => Object.fromEntries([...ids].map((id) => [String(id), ""]));
      if (wanted.skill.size) skillNames([...wanted.skill]).then((found) => setNames((current) => ({ ...current, skills: { ...current.skills, ...misses(wanted.skill), ...found } }))).catch(() => {});
      if (wanted.element.size || wanted.task.size) dynTaskLabels([...wanted.element], [...wanted.task]).then((found) => setNames((current) => ({ ...current, elements: { ...current.elements, ...misses(wanted.element), ...found.elements }, tasks: { ...current.tasks, ...misses(wanted.task), ...found.tasks } }))).catch(() => {});
    }
  }, [policy]);
  // Another elements.data names other things.
  useEffect(() => setNames((current) => ({ ...current, elements: {}, tasks: {} })), [elementsPath]);

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
  const monstersOf = useCallback((id: number) => usedBy[String(id)] ?? [], [usedBy]);
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

  // ── Editing ──

  const matchesRef = useRef(matches);
  matchesRef.current = matches;
  /** Shows what an edit returned: the list (or the changed policy's row), the policy and a trigger. */
  const applyResult = useCallback((result: AiEditResult, nextTrigger?: number | null) => {
    const shown = result.policy;
    setFile((current) => {
      if (!current) return current;
      if (result.file) return result.file;
      if (!shown) return { ...current, state: result.state };
      const operations = shown.triggers.flatMap((entry) => entry.operations);
      return { ...current, state: result.state, policies: current.policies.map((entry) => entry.index === shown.index ? { ...entry, triggers: shown.triggers.length, operations: operations.length, talks: operations.filter((operation) => operation.kind === 2 || operation.kind === 31 || operation.kind === 32).length, changed: true } : entry) };
    });
    if (!shown) { setSelected(null); setPolicy(null); setTrigger(null); return; }
    if (shown.index !== selectedRef.current) delivered.current = shown.index;
    setSelected(shown.index);
    setPolicy(shown);
    if (matchesRef.current === null) setPage(Math.floor(shown.index / PAGE));
    setTrigger((current) => {
      const at = nextTrigger !== undefined ? nextTrigger : current;
      return shown.triggers.length ? Math.max(0, Math.min(at ?? 0, shown.triggers.length - 1)) : null;
    });
  }, []);
  const run = useCallback(async (action: () => Promise<AiEditResult>, nextTrigger?: number | null) => {
    setBusy(true);
    setError(null);
    try {
      applyResult(await action(), nextTrigger);
    } catch (problem) {
      setError(messageOf(problem));
    } finally {
      setBusy(false);
    }
  }, [applyResult]);

  const applyTrigger = useCallback((edit: AiTriggerEdit) => {
    if (!policy || trigger === null) return;
    void run(() => aiPolicySetTrigger(policy.index, trigger, edit));
  }, [policy, run, trigger]);
  const triggerAction = useCallback((action: AiListAction) => {
    if (!policy || trigger === null || busy) return;
    const entry = policy.triggers[trigger];
    if (!entry) return;
    if (action.op === "move" && (action.to < 0 || action.to >= policy.triggers.length)) return;
    if (action.op === "delete") {
      const callers = entry.calledBy.filter((id) => id !== entry.id);
      if (!window.confirm(`Delete trigger ${entry.id}${entry.name ? ` (${entry.name})` : ""}?${callers.length ? `\n\nTriggers ${callers.join(", ")} run, enable or disable it; they would name a missing trigger.` : ""}\n\nUndo brings it back.`)) return;
    }
    void run(() => aiPolicyTriggerAction(policy.index, trigger, action), action.op === "clone" ? trigger + 1 : action.op === "move" ? action.to : trigger);
  }, [busy, policy, run, trigger]);
  const policyAction = useCallback((op: "clone" | "delete") => {
    if (!policy || busy || !file) return;
    if (op === "delete") {
      const monsters = monstersOf(policy.id);
      const others = file.policies.filter((entry) => entry.id === policy.id && entry.index !== policy.index).length;
      const users = monsters.length ? `\n\n${count(monsters.length)} monster${monsters.length === 1 ? " uses" : "s use"} it (${monsters.slice(0, 4).map(([id, name]) => `${name || "?"} ${id}`).join(", ")}${monsters.length > 4 ? ", …" : ""})${policy.shadowedBy === null && !others ? ": they would have no AI policy." : "."}` : "";
      if (!window.confirm(`Delete policy ${policy.id} with its ${count(policy.triggers.length)} triggers?${users}\n\nUndo brings it back.`)) return;
    }
    void run(() => aiPolicyPolicyAction(policy.index, { op }), 0);
  }, [busy, file, monstersOf, policy, run]);
  const undo = useCallback(() => { if (file?.state.canUndo && !busy) void run(aiPolicyUndo); }, [busy, file?.state.canUndo, run]);
  const redo = useCallback(() => { if (file?.state.canRedo && !busy) void run(aiPolicyRedo); }, [busy, file?.state.canRedo, run]);

  const saveTo = useCallback(async (target: string | null, replaceChanged = false): Promise<void> => {
    if (!file) return;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      if (!replaceChanged) {
        const found = await aiPolicyProblems();
        const errors = found.filter((problem) => problem.severity === "error");
        if (errors.length) {
          const lines = errors.slice(0, 8).map((problem) => `• ${problem.policyId !== null ? `Policy ${problem.policyId}${problem.triggerId !== null ? `, trigger ${problem.triggerId}` : ""}: ` : ""}${problem.message}`).join("\n");
          if (!window.confirm(`The server would crash or stop loading with this file:\n\n${lines}${errors.length > 8 ? `\n… and ${count(errors.length - 8)} more` : ""}\n\nSave anyway?`)) {
            setProblems(found);
            setProblemsOpen(true);
            return;
          }
        }
      }
      const report = await aiPolicySave(target, backup, replaceChanged);
      const view = await aiPolicyView();
      if (view) setFile(view);
      setNote(`Saved ${bytes(report.size)} to ${report.path}.${report.backup ? ` Backup: ${report.backup}.` : ""} The server reads aipolicy.data when it starts: restart it to use the changes.`);
    } catch (problem) {
      const message = messageOf(problem);
      if (message.startsWith("CHANGED_ON_DISK:") && window.confirm(`${message.slice("CHANGED_ON_DISK:".length).trim()}\n\nReplace it with the open policies?`)) {
        await saveTo(target, true);
        return;
      }
      setError(message);
    } finally {
      setBusy(false);
    }
  }, [backup, file]);
  const saveCurrent = useCallback(() => { if (file && dirty && !busy) void saveTo(null); }, [busy, dirty, file, saveTo]);
  const saveAs = useCallback(async () => {
    if (!file || busy) return;
    const target = await save({ defaultPath: file.path, title: "Save aipolicy.data as", filters: [{ name: "aipolicy.data", extensions: ["data"] }] });
    if (typeof target === "string") await saveTo(target);
  }, [busy, file, saveTo]);

  const checkProblems = useCallback(async () => {
    if (!file) return;
    setProblemsOpen(true);
    setProblems(null);
    try {
      setProblems(await aiPolicyProblems());
    } catch (problem) {
      setProblemsOpen(false);
      setError(messageOf(problem));
    }
  }, [file]);
  const showProblem = (problem: AiProblem) => {
    if (problem.policy === null || !file) return;
    const summary = file.policies[problem.policy];
    if (!summary) return;
    setQuery("");
    setMatches(null);
    setPage(Math.floor(problem.policy / PAGE));
    if (summary.index === selected && policy) {
      if (problem.trigger !== null) setTrigger(problem.trigger);
    } else {
      wantedTrigger.current = problem.trigger;
      setSelected(summary.index);
    }
    setProblemsOpen(false);
    window.setTimeout(() => document.querySelector(".ai-policy-row.selected")?.scrollIntoView({ block: "center" }), 50);
  };

  const cleanup = useCallback(async (whole: boolean) => {
    if (!file || busy || (!whole && !policy)) return;
    const scope = whole ? null : policy!.index;
    try {
      const found = await aiPolicyUnusedSubtriggers(scope);
      const total = found.reduce((sum, [, , ids]) => sum + ids.length, 0);
      if (!total) {
        setNote(whole ? "No policy has sub-triggers that nothing runs." : `Every sub-trigger of policy ${policy!.id} is run by another trigger.`);
        return;
      }
      const sample = found.slice(0, 6).map(([, id, ids]) => `policy ${id}: ${ids.join(", ")}`).join("\n");
      if (!window.confirm(`Remove ${count(total)} sub-trigger${total === 1 ? "" : "s"} that nothing runs${whole ? ` from ${count(found.length)} polic${found.length === 1 ? "y" : "ies"}` : ""}? No top-level trigger reaches them through "Run a trigger", so the server never runs them.\n\n${sample}${found.length > 6 ? "\n…" : ""}\n\nUndo brings them back.`)) return;
      await run(() => aiPolicyCleanup(scope, policy?.index ?? null));
    } catch (problem) {
      setError(messageOf(problem));
    }
  }, [busy, file, policy, run]);

  useImperativeHandle(ref, () => ({
    choose: () => void choose(),
    openPath: (path) => void load(path),
    showPolicy: (id) => setWanted(id),
    save: saveCurrent,
    saveAs: () => void saveAs(),
    undo,
    redo,
    triggerAction,
    policyAction,
    openProblems: () => void checkProblems(),
    openSimulator: () => { if (policy) setSimOpen(true); },
    openFlow: () => { if (policy) setFlowOpen(true); },
    cleanup: (whole) => void cleanup(whole),
  }), [checkProblems, choose, cleanup, load, policy, policyAction, redo, saveAs, saveCurrent, triggerAction, undo]);

  useEffect(() => {
    if (!active || !file) return;
    const onKey = (event: KeyboardEvent) => {
      const mod = event.ctrlKey || event.metaKey;
      const typing = event.target instanceof HTMLElement && /^(INPUT|TEXTAREA|SELECT)$/.test(event.target.tagName);
      const key = event.key.toLowerCase();
      if (event.key === "Escape" && (problemsOpen || simOpen || flowOpen)) { event.preventDefault(); setProblemsOpen(false); setSimOpen(false); setFlowOpen(false); return; }
      if (picking || simOpen || flowOpen) return;
      if (mod && event.shiftKey && key === "m") { event.preventDefault(); event.stopImmediatePropagation(); void checkProblems(); return; }
      if (mod && ["o", "s", "z", "y", "d"].includes(key)) {
        if (typing && (key === "z" || key === "y")) return;
        event.preventDefault();
        event.stopImmediatePropagation();
        if (key === "o") void choose();
        else if (key === "s") event.shiftKey ? void saveAs() : saveCurrent();
        else if (key === "z") event.shiftKey ? redo() : undo();
        else if (key === "y") redo();
        else triggerAction({ op: "clone" });
      } else if (event.key === "Delete" && !typing) {
        event.preventDefault();
        triggerAction({ op: "delete" });
      } else if (event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown") && !typing && trigger !== null) {
        event.preventDefault();
        triggerAction({ op: "move", to: trigger + (event.key === "ArrowUp" ? -1 : 1) });
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [active, checkProblems, choose, file, flowOpen, picking, problemsOpen, redo, simOpen, saveAs, saveCurrent, trigger, triggerAction, undo]);

  const showTrigger = (id: number) => {
    const found = policy?.triggers.findIndex((entry) => entry.id === id) ?? -1;
    if (found >= 0) setTrigger(found);
  };
  const triggerLink = (id: number) => {
    const target = byId.get(id);
    return <button className="link mono" onClick={() => showTrigger(id)} title={target ? target.name || target.expression : "No trigger with this ID in the policy: the server does nothing"}>{id}{target?.name ? ` · ${target.name}` : target ? "" : " (missing)"}</button>;
  };

  /** The name of what a parameter refers to (next to its input). */
  const nameOf = (param: AiParam): ReactNode => {
    const value = param.value;
    if (typeof value !== "number") return null;
    switch (param.refers) {
      case "skill": return value ? names.skills[String(value)] || <span className="muted small">unknown skill</span> : null;
      case "monster": case "item": case "mine": return value ? names.elements[String(value)] ? names.elements[String(value)].split(" › ").pop() : elementsPath ? <span className="ai-warn small">not in elements.data</span> : null : <span className="muted small">none</span>;
      case "task": return value ? names.tasks[String(value)] || null : null;
      case "timer": {
        const info = timers.get(value);
        return info ? <span className="muted small" title={`Tested by ${info.fires.join(", ") || "none"}`}>started by {info.starts.join(", ") || <span className="ai-warn">none</span>}{info.stops.length ? `; stopped by ${info.stops.join(", ")}` : ""}</span> : null;
      }
      default: return param.label.includes("(s") && value >= 60 ? <span className="muted small">{formatDuration(value)}</span> : null;
    }
  };
  const paramText = (param: AiParam): ReactNode => {
    const value = param.value;
    if (typeof value === "boolean") return value ? "yes" : "no";
    if (typeof value === "string") return value || "(empty)";
    if (param.key === "ratio" || param.key === "chance") return `${Math.round(value * 1000) / 10}%`;
    if (param.refers === "trigger") { const target = byId.get(value); return `${value}${target?.name ? ` · ${target.name}` : target ? "" : " (missing)"}`; }
    if (param.refers === "attack") return ATTACK_TYPES[value] ?? value;
    if (param.refers === "path_type") return PATH_TYPES[value] ?? value;
    if (param.refers === "global") return `global[${value}]`;
    return value;
  };
  const describeOperation = (operation: AiOperation): ReactNode => {
    const text = operation.params.find((param) => param.kind === "text");
    return <span className="ai-sim-op"><b>{operation.name}</b>{operation.usesTarget && <span className="muted small"> → {operation.targetName}</span>}{text ? <> “{String(text.value)}”</> : operation.params.map((param) => { const name = param.refers && param.refers !== "trigger" && param.refers !== "timer" ? nameOf(param) : null; return <span key={param.key} className="ai-param"><span className="muted small">{param.label}</span> {paramText(param)}{name && <> <span className="ai-name">{name}</span></>}</span>; })}</span>;
  };
  const formContext: FormContext | null = catalog && policy ? { catalog, triggers: policy.triggers, busy, nameOf, pick: (refers, current, apply) => setPicking({ refers, current, apply }) } : null;
  const pickerSearch = useMemo(() => picking ? (text: string, page: number): Promise<PickerResult> => picking.refers === "skill" ? pickSkill(text, page, picking.current || null) : pickEssence(picking.refers as "monster" | "item" | "mine", text, page, picking.current || null) : undefined, [picking]);

  if (!file) return <section className="path-data-pane empty">
    <div className="drop-card path-data-empty">
      <Brain size={30} />
      <h2>Open aipolicy.data</h2>
      <p className="muted">The monster AI the server runs (<span className="mono">PolicyData</span> in gs.conf): policies of triggers, each a condition and the operations it runs. A monster uses the policy of its <span className="mono">common_strategy</span> (fields with the <i>aipolicy</i> role in the schema editor).</p>
      <button className="btn primary" onClick={() => void choose()} disabled={busy}>{busy ? <Loader2 size={14} className="spin" /> : <FolderOpen size={14} />} Choose aipolicy.data…</button>
      {defaultPath && <button className="btn" onClick={() => void load(defaultPath)} disabled={busy}>Open the configured client's file</button>}
      {error && <div className="se-problems" role="alert">{error}</div>}
    </div>
  </section>;

  const lastStep = file.state.history.filter((row) => !row.undone).pop();
  const nextStep = file.state.history.find((row) => row.undone);
  return <section className="dyn-tasks-pane" aria-busy={busy}>
    <header className="tasks-head">
      <div><h2>AI policies {dirty && <span className="tag warn">unsaved</span>}{catalog && <span className="tag" title={catalog.newer ? "The file has version-12 triggers or types only newer servers read: all types are offered" : "Version-11 triggers (the 2013 server source): types that server does not read are not offered"}>{catalog.newer ? "newer server" : "2013 server"}</span>}</h2><div className="tasks-file-line">
        <span className="mono truncate" title={file.path}>{file.path.split(/[\\/]/).slice(-3).join("/")}</span>
        <span className="path-data-badge"><b>Policies:</b> {count(file.policies.length)}</span>
        <span className="path-data-badge"><b>Triggers:</b> {count(file.triggers)}</span>
        <span className="path-data-badge" title="Trigger versions: 11 is the 2013 server source's, 12 the newer server's (more conditions and operations)"><b>Versions:</b> {file.versions.map(([version, number]) => `${version} (${count(number)})`).join(", ")}</span>
        <span className="path-data-badge"><b>Size:</b> {bytes(file.size)}</span>
      </div></div>
      <button className="btn" onClick={undo} disabled={busy || !file.state.canUndo} title={lastStep ? `Undo: ${lastStep.label} (Ctrl+Z)` : "Undo (Ctrl+Z)"}><Undo2 size={14} /></button>
      <button className="btn" onClick={redo} disabled={busy || !file.state.canRedo} title={nextStep ? `Redo: ${nextStep.label} (Ctrl+Y)` : "Redo (Ctrl+Y)"}><Redo2 size={14} /></button>
      <button className="btn" onClick={() => void checkProblems()} disabled={busy} title="Check problems (Ctrl+Shift+M)"><CircleAlert size={14} /> Problems</button>
      <button className="btn primary" onClick={saveCurrent} disabled={busy || !dirty} title="Save (Ctrl+S)"><Save size={14} /> Save</button>
      <label className="check" title="Before the first save over a file, archive it in jdide_backups next to it"><input type="checkbox" checked={backup} onChange={(event) => { setBackup(event.target.checked); try { localStorage.setItem(BACKUP_KEY, event.target.checked ? "1" : "0"); } catch { /* optional */ } }} /> Backup</label>
      <button className="btn small" onClick={() => void saveAs()} disabled={busy}>Save as…</button>
      <button className="btn" onClick={() => void choose()} disabled={busy}><FolderOpen size={14} /> Open…</button>
    </header>
    {error && <div className="path-data-message error"><AlertTriangle size={14} /> {error} <button className="link" onClick={() => setError(null)}>Dismiss</button></div>}
    {note && <div className="path-data-message"><Info size={14} /> {note} <button className="link" onClick={() => setNote(null)}>Dismiss</button></div>}
    {!elementsPath && <div className="path-data-message">Open the server's elements.data to see which monsters use each policy and the names of monsters, items and tasks.</div>}
    <div className="ai-body">
      <aside className="dyn-task-list">
        <div className="dyn-task-search"><Search size={13} /><input value={query} placeholder="Policy or monster ID, name, text, skill ID…" onChange={(event) => setQuery(event.target.value)} />{searching ? <Loader2 size={12} className="spin" /> : query && <button className="icon-btn small" onClick={() => setQuery("")}><X size={12} /></button>}</div>
        <div className="dyn-task-rows" role="listbox" ref={rowsRef}>
          {pageRows.map((entry) => {
            const monsters = monstersOf(entry.id);
            return <button key={entry.index} role="option" aria-selected={entry.index === selected} className={"dyn-task-row ai-policy-row" + (entry.index === selected ? " selected" : "")} onClick={() => setSelected(entry.index)}>
              <span className="mono">{entry.changed && <span className="changed-dot" title="Changed since opened or saved" />}{entry.id}</span>
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
            <span className="spacer" />
            <button className="icon-btn small" onClick={() => setFlowOpen(true)} title="Flow: which triggers run, enable or disable others and which start timers"><Network size={13} /></button>
            <button className="icon-btn small" onClick={() => setSimOpen(true)} title="Simulate the policy as the server runs it"><Play size={13} /></button>
            <button className="icon-btn small" onClick={() => void cleanup(false)} disabled={busy} title="Remove sub-triggers nothing runs"><Eraser size={13} /></button>
            <button className="icon-btn small" onClick={() => policyAction("clone")} disabled={busy} title="Clone the policy (next free policy ID); point a monster's common_strategy at the new ID to use it"><Copy size={13} /></button>
            <button className="icon-btn small danger" onClick={() => policyAction("delete")} disabled={busy} title="Delete the policy"><Trash2 size={13} /></button>
            <div className="muted small ai-used-by">{monstersOf(policy.id).length ? <>Used by {monstersOf(policy.id).slice(0, 6).map(([id, name]) => `${name || "?"} (${id})`).join(", ")}{monstersOf(policy.id).length > 6 ? ` and ${monstersOf(policy.id).length - 6} more` : ""}</> : elementsPath ? "No monster in the open elements.data uses it." : null}</div>
          </div>
          <div className="ai-trigger-tools">
            <span className="muted small">Triggers</span>
            <span className="spacer" />
            <button className="icon-btn small" onClick={() => triggerAction({ op: "clone" })} disabled={busy || trigger === null} title="Clone the trigger below (next free trigger ID) (Ctrl+D)"><Copy size={13} /></button>
            <button className="icon-btn small" onClick={() => trigger !== null && triggerAction({ op: "move", to: trigger - 1 })} disabled={busy || trigger === null || trigger === 0} title="Move up (Alt+↑)"><ArrowUp size={13} /></button>
            <button className="icon-btn small" onClick={() => trigger !== null && triggerAction({ op: "move", to: trigger + 1 })} disabled={busy || trigger === null || trigger + 1 >= policy.triggers.length} title="Move down (Alt+↓)"><ArrowDown size={13} /></button>
            <button className="icon-btn small danger" onClick={() => triggerAction({ op: "delete" })} disabled={busy || trigger === null} title="Delete the trigger (Del)"><Trash2 size={13} /></button>
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
            {!policy.triggers.length && <div className="empty-note">No triggers.</div>}
          </div>
        </>}
      </aside>
      <div className="dyn-task-form-scroll">
        {!selectedTrigger ? <div className="empty-note">{policy ? "Select a trigger." : "Policies are lists of triggers. A trigger runs its operations when its condition holds; which list it is tested in depends on its first condition (birth, combat start, timer, heartbeat, …)."}</div> : <>
          <div className="dyn-task-title"><h3>Trigger {selectedTrigger.id}</h3><span className="muted">{selectedTrigger.name}</span><span className="muted small">version {selectedTrigger.version}</span></div>
          <div className="ai-flags">
            {selectedTrigger.run ? <span className="tag" title="Not tested on its own: another trigger's 'Run a trigger' runs it">Sub-trigger</span> : <span className="tag" title={CATEGORY_HINTS[selectedTrigger.category]}>{selectedTrigger.category}</span>}
            {selectedTrigger.firesOnce && <span className="tag" title="HP below, aggro count, distance and skill hit disable their trigger after it fires (until the monster resets)">Fires once</span>}
            {selectedTrigger.selectOne && <span className="tag">One random operation</span>}
          </div>
          {selectedTrigger.calledBy.length > 0 && <p className="small">Run, enabled or disabled by {selectedTrigger.calledBy.map((id, index) => <span key={id}>{index ? ", " : ""}{triggerLink(id)}</span>)}</p>}
          {selectedTrigger.run && !selectedTrigger.calledBy.length && <p className="small ai-warn">No trigger of this policy runs it: it never runs (the official editor's cleanup removes such triggers).</p>}
          {formContext ? <AiTriggerForm key={`${policy!.index}:${selectedTrigger.index}`} trigger={selectedTrigger} ctx={formContext} onApply={applyTrigger} /> : <div className="empty-note"><Loader2 size={14} className="spin" /> Loading the types…</div>}
        </>}
      </div>
    </div>
    {simOpen && policy && <AiSimulator policy={policy} newer={!!catalog?.newer} describe={describeOperation} onShowTrigger={(index) => { setTrigger(index); setSimOpen(false); }} onClose={() => setSimOpen(false)} />}
    {flowOpen && policy && <AiPolicyFlow policy={policy} onShowTrigger={(index) => { setTrigger(index); setFlowOpen(false); }} onClose={() => setFlowOpen(false)} />}
    {picking && <ValuePicker search={pickerSearch} onApply={async (value) => { const id = Number(value); if (!Number.isInteger(id) || id < 0) return "Choose an entry"; picking.apply(id); setPicking(null); return null; }} onClose={() => setPicking(null)} />}
    {problemsOpen && (() => {
      const errors = (problems ?? []).filter((problem) => problem.severity === "error").length;
      const warnings = (problems ?? []).filter((problem) => problem.severity === "warning").length;
      const notes = (problems ?? []).length - errors - warnings;
      return <div className="modal-backdrop" onMouseDown={() => setProblemsOpen(false)}>
        <div className="modal npcgen-problems-dialog" role="dialog" aria-label="Problems" onMouseDown={(event) => event.stopPropagation()}>
          <header className="modal-head"><h3>Problems</h3>{problems && <span className="muted small">{count(errors)} error{errors === 1 ? "" : "s"} · {count(warnings)} warning{warnings === 1 ? "" : "s"} · {count(notes)} note{notes === 1 ? "" : "s"}</span>}<span className="spacer" /><button className="btn small" onClick={() => void checkProblems()} disabled={!problems}>Check again</button><button className="icon-btn" onClick={() => setProblemsOpen(false)} aria-label="Close"><X size={16} /></button></header>
          <div className="npcgen-problems-list">
            {!problems ? <div className="empty-note">Checking…</div> : !problems.length ? <div className="empty-note">No problems found.</div> : problems.map((problem, index) => <button key={index} className={"npcgen-problem " + problem.severity} onClick={() => showProblem(problem)} disabled={problem.policy === null}>
              {problem.severity === "error" ? <CircleAlert size={14} /> : problem.severity === "warning" ? <TriangleAlert size={14} /> : <Info size={14} />}
              <span>{problem.policyId !== null && <b>Policy {problem.policyId}{problem.triggerId !== null ? `, trigger ${problem.triggerId}` : ""}</b>} {problem.message}</span>
            </button>)}
          </div>
          <footer className="modal-foot muted small">Rules from the server source (gs policy loader and AI). Errors crash or stop the 2013 server; warnings misbehave; notes are probably unintended. {elementsPath ? "Monsters, items and mines are checked against the open elements.data." : "Open elements.data to check monsters, items and mines too."}</footer>
        </div>
      </div>;
    })()}
  </section>;
});
