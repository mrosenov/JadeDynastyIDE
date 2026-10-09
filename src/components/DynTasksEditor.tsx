import { forwardRef, useCallback, useDeferredValue, useEffect, useImperativeHandle, useMemo, useRef, useState, type ReactNode } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, CircleAlert, Copy, FolderOpen, Gift, History, Loader2, Plus, Redo2, Save, Search, Trash2, Undo2, X } from "lucide-react";
import { cloneDynTask, deleteDynTask, dynTaskLabels, dynTaskProblems, dynTasksView, getDynTask, openDynTasks, redoDynTask, saveDynTasks, setDynTask, undoDynTask } from "../elements/api";
import { bytes, count } from "../elements/format";
import { formatMoney, parseMoney } from "../elements/money";
import { formatDuration } from "../elements/time";
import type { DynCandidate, DynItem, DynLabels, DynMonster, DynProblemReport, DynTalk, DynTask, DynTaskTime, DynTimetableEntry, DynVert, DynView, TaskDialog } from "../elements/types";
import { DialogTreeEditor } from "./TaskDialogEditor";
import { loadSet } from "./TaskForm";

export interface DynTasksEditorState {
  loaded: boolean;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  path: string | null;
  tasks: number;
  timeMark: number | null;
  panel: DynPanel;
}

export interface DynTasksEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
  save: () => void;
  saveAs: () => void;
  undo: () => void;
  redo: () => void;
  cloneSelected: () => void;
  deleteSelected: () => void;
  toggleProblems: () => void;
  toggleHistory: () => void;
}

type DynPanel = "problems" | "history" | null;
type Tab = "general" | "requirements" | "goal" | "rewards" | "texts" | "dialogs";

interface Props {
  active: boolean;
  onStateChange: (state: DynTasksEditorState) => void;
}

const TABS: { key: Tab; label: string }[] = [
  { key: "general", label: "General" },
  { key: "requirements", label: "Requirements" },
  { key: "goal", label: "Goal" },
  { key: "rewards", label: "Rewards" },
  { key: "texts", label: "Texts" },
  { key: "dialogs", label: "Dialogs" },
];
/** The 17 flag bytes in pack order (`UnmarshalDynTask`). */
const FLAGS = ["Choose one subtask", "Random subtask", "Subtasks in order", "Parent also fails", "Parent also succeeds", "Can give up", "Can redo", "Can redo after failure", "Clear as give up", "Record", "Fail on death", "Auto deliver", "Death trigger", "Clear acquired items", "Needs spouse", "Teamwork", "Show direction"];
const TALK_KEYS: TaskDialog["talk"][] = ["delivery", "unqualified", "item_delivery", "execution", "award"];
const FUNCTION = 0x80000000;
const ROOT = 0xffffffff;
const QUEST_FUNCTIONS = new Set([0, 6, 7, 8, 21]);
const METHODS_WITH_DATA = new Set([1, 2, 4, 5, 13]);
const LIMITS = { name: 29, premiseTasks: 5, mutexTasks: 5, occupations: 45, timetable: 12, monsters: 3, itemsWanted: 8, candidates: 16, awardItems: 32, list: 255 };
const U8 = 255, U16 = 65535, U32 = 4294967295, I32_MIN = -2147483648, I32_MAX = 2147483647;
const newItem = (): DynItem => ({ itemId: 0, commonItem: 1, amount: 1, probability: 1, bound: 0, period: 0, timetable: 0, dayOfWeek: 0, hour: 0, minute: 0, refineCondition: 0, refineLevel: 0, replacementItemId: 0 });
const newMonster = (): DynMonster => ({ monsterId: 0, amount: 1, dropItemId: 0, dropItemAmount: 0, dropCommonItem: 0, dropProbability: 1, killerLevel: 0 });
const origin = (): DynVert => ({ x: 0, y: 0, z: 0 });
const noTime = (): DynTaskTime => ({ year: 0, month: 0, day: 0, hour: 0, minute: 0, weekday: 0 });

/** The shortest decimal that is the same 32-bit float. */
function f32(value: number) {
  for (let digits = 1; digits <= 9; digits++) {
    const text = Number(value.toPrecision(digits));
    if (Math.fround(text) === value) return String(text);
  }
  return String(value);
}

function nodeAt(task: DynTask, path: number[]): DynTask {
  return path.reduce((node, index) => node.subtasks[index], task);
}
function withNode(task: DynTask, path: number[], node: DynTask): DynTask {
  if (!path.length) return node;
  const [head, ...rest] = path;
  return { ...task, subtasks: task.subtasks.map((child, index) => index === head ? withNode(child, rest, node) : child) };
}
function nodes(task: DynTask, path: number[] = [], depth = 0): { path: number[]; task: DynTask; depth: number }[] {
  return [{ path, task, depth }, ...task.subtasks.flatMap((child, index) => nodes(child, [...path, index], depth + 1))];
}

/** Talks as the shared dialog editor sees them: window texts without their stored NUL. */
function toDialogs(talks: DynTalk[]): TaskDialog[] {
  return talks.map((talk, index) => ({
    talk: TALK_KEYS[index],
    prompt: talk.prompt,
    windows: talk.windows.map((window) => ({ id: window.id, parentId: window.parent < 0 ? ROOT : window.parent, text: window.text.replace(/\0+$/, "").replace(/\r\n/g, "\n"), options: window.options.map((option) => ({ target: option.id >>> 0, text: option.text, parameter: option.param >>> 0 })) })),
  }));
}

/** Back to the pack: windows depth first from the first one, parents from the tree, texts ending with a NUL. */
function fromDialog(dialog: TaskDialog): DynTalk {
  if (!dialog.windows.length) return { prompt: dialog.prompt, windows: [] };
  const byId = new Map(dialog.windows.map((window) => [window.id, window]));
  if (byId.size !== dialog.windows.length) throw new Error("Two windows have the same ID");
  const out: DynTalk["windows"] = [];
  const seen = new Set<number>();
  const visit = (id: number, parent: number) => {
    const window = byId.get(id);
    if (!window) throw new Error(`An option opens window ${id}, which does not exist`);
    if (id < 0 || id > 127) throw new Error(`Window ${id}: dyn_tasks.data stores window IDs as one signed byte (0–127)`);
    if (seen.has(id)) throw new Error(`Window ${id} is opened by more than one option`);
    seen.add(id);
    out.push({ id, parent, text: window.text.replace(/\r?\n/g, "\r\n") + "\0", options: window.options.map((option) => ({ id: option.target >>> 0, param: option.parameter >>> 0, text: option.text })) });
    for (const option of window.options) if (option.target < FUNCTION) visit(option.target, id);
  };
  visit(dialog.windows[0].id, -1);
  const lost = dialog.windows.find((window) => !seen.has(window.id));
  if (lost) throw new Error(`Window ${lost.id} is not opened by any option`);
  return { prompt: dialog.prompt, windows: out };
}

/** IDs of items, monsters and tasks a task names (for their labels). */
function referencedIds(task: DynTask) {
  const elements = new Set<number>();
  const tasks = new Set<number>();
  for (const { task: node } of nodes(task)) {
    const items = [...(node.premiseItems ?? []), ...(node.givenItems?.items ?? []), ...node.goal.items, ...(node.award.candidates ?? []).flatMap((candidate) => candidate.items)];
    for (const item of items) { elements.add(item.itemId); elements.add(item.replacementItemId); }
    for (const monster of node.goal.monsters) { elements.add(monster.monsterId); elements.add(monster.dropItemId); }
    for (const id of [...(node.premiseTasks ?? []), ...(node.mutexTasks ?? [])]) tasks.add(id);
    for (const talk of node.talks) for (const window of talk.windows) for (const option of window.options) {
      if (option.id >= FUNCTION && QUEST_FUNCTIONS.has(option.id - FUNCTION)) tasks.add(option.param);
    }
  }
  elements.delete(0);
  tasks.delete(0);
  return { elements: [...elements], tasks: [...tasks] };
}

// ── Small inputs that apply on Enter or when they lose focus ──

function NumberInput({ value, onCommit, min = 0, max = U32, float, title, placeholder, wide }: { value: number; onCommit: (value: number) => void; min?: number; max?: number; float?: boolean; title?: string; placeholder?: string; wide?: boolean }) {
  const shown = float ? f32(value) : String(value);
  const [draft, setDraft] = useState(shown);
  useEffect(() => setDraft(shown), [shown]);
  const commit = () => {
    const text = draft.trim();
    const number = Number(text);
    if (!text || !Number.isFinite(number) || (!float && !Number.isInteger(number)) || number < min || number > max) { setDraft(shown); return; }
    if (float ? Math.fround(number) !== value : number !== value) onCommit(float ? Math.fround(number) : number);
  };
  return <input className={"dyn-input mono" + (wide ? " wide" : "")} value={draft} title={title ?? `${min} – ${max}`} placeholder={placeholder} onChange={(event) => setDraft(event.target.value)} onBlur={commit} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); if (event.key === "Escape") setDraft(shown); }} />;
}

function MoneyInput({ value, onCommit }: { value: number; onCommit: (value: number) => void }) {
  const shown = formatMoney(value);
  const [draft, setDraft] = useState(shown);
  useEffect(() => setDraft(shown), [shown]);
  const commit = () => {
    const parsed = parseMoney(draft);
    if (parsed === null || parsed < 0 || parsed > U32) { setDraft(shown); return; }
    if (parsed !== value) onCommit(parsed);
  };
  return <input className="dyn-input mono wide" value={draft} title="Copper; 1G 50S or 1 Gold 50 Silver also work" onChange={(event) => setDraft(event.target.value)} onBlur={commit} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); if (event.key === "Escape") setDraft(shown); }} />;
}

function TextInput({ value, max, multiline, placeholder, onCommit }: { value: string; max?: number; multiline?: boolean; placeholder?: string; onCommit: (value: string) => void }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => { if (draft !== value) onCommit(draft); };
  return multiline
    ? <textarea className="dyn-textarea" value={draft} rows={Math.min(10, Math.max(3, draft.split("\n").length + 1))} placeholder={placeholder} onChange={(event) => setDraft(event.target.value)} onBlur={commit} onKeyDown={(event) => { if (event.key === "Escape") setDraft(value); }} />
    : <input className="dyn-input wide" value={draft} maxLength={max} placeholder={placeholder} onChange={(event) => setDraft(event.target.value)} onBlur={commit} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); if (event.key === "Escape") setDraft(value); }} />;
}

function Select({ value, set, onCommit, fallback }: { value: number; set?: Map<number, string>; onCommit: (value: number) => void; fallback: string }) {
  const entries = [...(set ?? new Map<number, string>()).entries()].sort((a, b) => a[0] - b[0]);
  if (!entries.some(([key]) => key === value)) entries.push([value, `${fallback} ${value}`]);
  return <select className="task-form-select" value={value} onChange={(event) => onCommit(Number(event.target.value))}>{entries.map(([key, label]) => <option key={key} value={key}>{key} · {label}</option>)}</select>;
}

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return <label className="dyn-field" title={hint}><span className="task-form-label">{label}</span><span className="dyn-field-value">{children}</span></label>;
}

/** An optional section: stored only when present. */
function Optional({ title, present, onAdd, onRemove, hint, children }: { title: string; present: boolean; onAdd: () => void; onRemove: () => void; hint?: string; children?: ReactNode }) {
  return <section className={"dyn-section" + (present ? "" : " absent")}>
    <header><b>{title}</b>{hint && <span className="muted small">{hint}</span>}<span className="spacer" />
      {present ? <button className="btn small" onClick={onRemove} title="Remove it from this task"><X size={12} /> Remove</button> : <button className="btn small" onClick={onAdd}><Plus size={12} /> Add</button>}
    </header>
    {present && <div className="dyn-section-body">{children}</div>}
  </section>;
}

function Label({ id, labels, kind }: { id: number; labels: DynLabels; kind: "elements" | "tasks" }) {
  const label = id ? labels[kind][String(id)] : undefined;
  return label ? <span className="dyn-label truncate" title={label}>{label}</span> : null;
}

function VertInput({ value, onCommit }: { value: DynVert; onCommit: (value: DynVert) => void }) {
  return <span className="dyn-vert">{(["x", "y", "z"] as const).map((axis) => <NumberInput key={axis} float min={-1e9} max={1e9} value={value[axis]} title={axis.toUpperCase()} onCommit={(number) => onCommit({ ...value, [axis]: number })} />)}</span>;
}

function TimeInput({ value, onCommit }: { value: DynTaskTime; onCommit: (value: DynTaskTime) => void }) {
  const parts: (keyof DynTaskTime)[] = ["year", "month", "day", "hour", "minute", "weekday"];
  return <span className="dyn-time">{parts.map((part) => <NumberInput key={part} min={I32_MIN} max={I32_MAX} value={value[part]} title={part} placeholder={part} onCommit={(number) => onCommit({ ...value, [part]: number })} />)}</span>;
}

function IdList({ ids, max, onCommit, render, placeholder }: { ids: number[]; max: number; onCommit: (ids: number[]) => void; render?: (id: number) => ReactNode; placeholder: string }) {
  const [draft, setDraft] = useState("");
  const add = () => {
    const value = Number(draft.trim());
    if (!Number.isInteger(value) || value <= 0 || value > U32) return;
    setDraft("");
    onCommit([...ids, value]);
  };
  return <div className="dyn-id-list">
    {ids.map((id, index) => <span className="dyn-id-chip" key={`${id}:${index}`}><span className="mono">{id}</span>{render?.(id)}<button className="icon-btn small" onClick={() => onCommit(ids.filter((_, position) => position !== index))} title="Remove"><X size={11} /></button></span>)}
    {ids.length < max && <span className="dyn-id-add"><input className="dyn-input mono" value={draft} placeholder={placeholder} onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") add(); }} /><button className="btn small" onClick={add} disabled={!draft.trim()}><Plus size={12} /></button></span>}
    <span className="muted small">{ids.length} / {max}</span>
  </div>;
}

function ItemTable({ items, max, labels, onCommit }: { items: DynItem[]; max: number; labels: DynLabels; onCommit: (items: DynItem[]) => void }) {
  const [all, setAll] = useState(false);
  const set = (index: number, change: Partial<DynItem>) => onCommit(items.map((item, position) => position === index ? { ...item, ...change } : item));
  const flag = (index: number, key: keyof DynItem, title: string) => <input type="checkbox" title={title} checked={items[index][key] !== 0} onChange={(event) => set(index, { [key]: event.target.checked ? 1 : 0 })} />;
  return <div className="dyn-table-wrap">
    <table className="dyn-table">
      <thead><tr><th>Item</th><th>Amount</th><th title="Common item (not a quest item)">Common</th><th>Probability</th><th>Bound</th><th title="Expires after (seconds)">Period</th>{all && <><th>Timetable</th><th>Day</th><th>Hour</th><th>Minute</th><th>Refine cond.</th><th>Refine level</th><th>Replacement</th></>}<th /></tr></thead>
      <tbody>{items.map((item, index) => <tr key={index}>
        <td><span className="dyn-item-cell"><NumberInput value={item.itemId} onCommit={(itemId) => set(index, { itemId })} /><Label id={item.itemId} labels={labels} kind="elements" /></span></td>
        <td><NumberInput value={item.amount} onCommit={(amount) => set(index, { amount })} /></td>
        <td className="center">{flag(index, "commonItem", "Common item")}</td>
        <td><NumberInput float min={0} max={1} value={item.probability} onCommit={(probability) => set(index, { probability })} /></td>
        <td className="center">{flag(index, "bound", "Bound to the player")}</td>
        <td><NumberInput min={I32_MIN} max={I32_MAX} value={item.period} title={item.period ? formatDuration(item.period) : "No expiry"} onCommit={(period) => set(index, { period })} /></td>
        {all && <>
          <td className="center">{flag(index, "timetable", "Expires at a time of the week")}</td>
          <td><NumberInput max={U8} value={item.dayOfWeek} onCommit={(dayOfWeek) => set(index, { dayOfWeek })} /></td>
          <td><NumberInput max={U8} value={item.hour} onCommit={(hour) => set(index, { hour })} /></td>
          <td><NumberInput max={U8} value={item.minute} onCommit={(minute) => set(index, { minute })} /></td>
          <td><NumberInput max={U8} value={item.refineCondition} onCommit={(refineCondition) => set(index, { refineCondition })} /></td>
          <td><NumberInput value={item.refineLevel} onCommit={(refineLevel) => set(index, { refineLevel })} /></td>
          <td><span className="dyn-item-cell"><NumberInput value={item.replacementItemId} onCommit={(replacementItemId) => set(index, { replacementItemId })} /><Label id={item.replacementItemId} labels={labels} kind="elements" /></span></td>
        </>}
        <td><button className="icon-btn small danger" onClick={() => onCommit(items.filter((_, position) => position !== index))} title="Remove item"><Trash2 size={12} /></button></td>
      </tr>)}</tbody>
    </table>
    <div className="dyn-table-foot">
      <button className="btn small" disabled={items.length >= max} onClick={() => onCommit([...items, newItem()])}><Plus size={12} /> Add item</button>
      <span className="muted small">{items.length} / {max}</span>
      <span className="spacer" />
      <label className="muted small"><input type="checkbox" checked={all} onChange={(event) => setAll(event.target.checked)} /> All columns</label>
    </div>
  </div>;
}

function MonsterTable({ monsters, labels, onCommit }: { monsters: DynMonster[]; labels: DynLabels; onCommit: (monsters: DynMonster[]) => void }) {
  const set = (index: number, change: Partial<DynMonster>) => onCommit(monsters.map((monster, position) => position === index ? { ...monster, ...change } : monster));
  return <div className="dyn-table-wrap">
    <table className="dyn-table">
      <thead><tr><th>Monster</th><th>Amount</th><th>Drop item</th><th>Drop amount</th><th>Common</th><th>Drop probability</th><th title="Only kills by players near the monster's level count">Level check</th><th /></tr></thead>
      <tbody>{monsters.map((monster, index) => <tr key={index}>
        <td><span className="dyn-item-cell"><NumberInput value={monster.monsterId} onCommit={(monsterId) => set(index, { monsterId })} /><Label id={monster.monsterId} labels={labels} kind="elements" /></span></td>
        <td><NumberInput value={monster.amount} onCommit={(amount) => set(index, { amount })} /></td>
        <td><span className="dyn-item-cell"><NumberInput value={monster.dropItemId} onCommit={(dropItemId) => set(index, { dropItemId })} /><Label id={monster.dropItemId} labels={labels} kind="elements" /></span></td>
        <td><NumberInput value={monster.dropItemAmount} onCommit={(dropItemAmount) => set(index, { dropItemAmount })} /></td>
        <td className="center"><input type="checkbox" checked={monster.dropCommonItem !== 0} onChange={(event) => set(index, { dropCommonItem: event.target.checked ? 1 : 0 })} /></td>
        <td><NumberInput float min={0} max={1} value={monster.dropProbability} onCommit={(dropProbability) => set(index, { dropProbability })} /></td>
        <td className="center"><input type="checkbox" checked={monster.killerLevel !== 0} onChange={(event) => set(index, { killerLevel: event.target.checked ? 1 : 0 })} /></td>
        <td><button className="icon-btn small danger" onClick={() => onCommit(monsters.filter((_, position) => position !== index))} title="Remove monster"><Trash2 size={12} /></button></td>
      </tr>)}</tbody>
    </table>
    <div className="dyn-table-foot"><button className="btn small" disabled={monsters.length >= LIMITS.monsters} onClick={() => onCommit([...monsters, newMonster()])}><Plus size={12} /> Add monster</button><span className="muted small">{monsters.length} / {LIMITS.monsters}</span></div>
  </div>;
}

// ── The form ──

interface FormProps {
  task: DynTask;
  top: boolean;
  tab: Tab;
  layout: DynView["layout"];
  labels: DynLabels;
  sets: Record<string, Map<number, string>>;
  onCommit: (task: DynTask, label: string) => void;
  onDialog: (dialog: TaskDialog, label: string) => Promise<void>;
}

function DynTaskForm({ task, top, tab, layout, labels, sets, onCommit, onDialog }: FormProps) {
  const set = (change: Partial<DynTask>, label: string) => onCommit({ ...task, ...change }, label);
  const taskLabel = (id: number) => <Label id={id} labels={labels} kind="tasks" />;
  const dialogs = useMemo(() => toDialogs(task.talks), [task.talks]);

  if (tab === "general") return <div className="dyn-form">
    <div className="dyn-grid">
      <Field label="ID" hint="Must be unique among dynamic tasks and the server's tasks.data"><NumberInput min={1} value={task.id} onCommit={(id) => set({ id }, "Change task ID")} /></Field>
      <Field label="Name" hint={`At most ${LIMITS.name} characters`}><TextInput value={task.name} max={LIMITS.name} onCommit={(name) => set({ name }, "Edit name")} /></Field>
      <Field label="Dynamic type"><Select value={task.dynType} set={sets.task_dynamic_type} fallback="Type" onCommit={(dynType) => set({ dynType }, "Edit dynamic type")} /></Field>
      {top && task.dynType === 1 && <Field label="Special award" hint="The number a player must have been given (by the server) to take this task"><NumberInput min={1} value={task.specialAward} onCommit={(specialAward) => set({ specialAward }, "Edit special award")} /></Field>}
      <Field label="Finish type"><Select value={task.finishType} set={sets.task_finish_type} fallback="Type" onCommit={(finishType) => set({ finishType }, "Edit finish type")} /></Field>
      <Field label="Level from"><NumberInput max={U8} value={task.levelMin} onCommit={(levelMin) => set({ levelMin }, "Edit level")} /></Field>
      <Field label="Level to" hint="0: no upper limit"><NumberInput max={U8} value={task.levelMax} onCommit={(levelMax) => set({ levelMax }, "Edit level")} /></Field>
    </div>
    <section className="dyn-section"><header><b>Flags</b></header>
      <div className="dyn-flags">{FLAGS.map((name, index) => <label key={name}><input type="checkbox" checked={task.flags[index] !== 0} onChange={(event) => set({ flags: task.flags.map((flag, position) => position === index ? (event.target.checked ? 1 : 0) : flag) }, `Edit ${name.toLowerCase()}`)} /> {name}</label>)}</div>
    </section>
  </div>;

  if (tab === "requirements") return <div className="dyn-form">
    <Optional title="Time limit" present={task.timeLimit !== null} onAdd={() => set({ timeLimit: 3600 }, "Add time limit")} onRemove={() => set({ timeLimit: null }, "Remove time limit")} hint={task.timeLimit ? formatDuration(task.timeLimit) : undefined}>
      <Field label="Seconds"><NumberInput value={task.timeLimit ?? 0} onCommit={(timeLimit) => set({ timeLimit }, "Edit time limit")} /></Field>
    </Optional>
    <Optional title="Reputation" present={task.reputation !== null} onAdd={() => set({ reputation: 0 }, "Add reputation requirement")} onRemove={() => set({ reputation: null }, "Remove reputation requirement")}>
      <Field label="At least"><NumberInput min={I32_MIN} max={I32_MAX} value={task.reputation ?? 0} onCommit={(reputation) => set({ reputation }, "Edit reputation requirement")} /></Field>
    </Optional>
    <Optional title="Period" present={task.period !== null} onAdd={() => set({ period: 0 }, "Add period")} onRemove={() => set({ period: null }, "Remove period")} hint="Cultivation stage">
      <Field label="Period"><NumberInput max={U16} value={task.period ?? 0} onCommit={(period) => set({ period }, "Edit period")} /></Field>
    </Optional>
    <Optional title="Gender" present={task.gender !== null} onAdd={() => set({ gender: 0 }, "Add gender requirement")} onRemove={() => set({ gender: null }, "Remove gender requirement")}>
      <Field label="Gender"><Select value={task.gender ?? 0} set={sets.task_gender} fallback="Gender" onCommit={(gender) => set({ gender }, "Edit gender requirement")} /></Field>
    </Optional>
    <Optional title="Deposit" present={task.deposit !== null} onAdd={() => set({ deposit: 0 }, "Add deposit")} onRemove={() => set({ deposit: null }, "Remove deposit")} hint="Money taken when the task is accepted">
      <Field label="Money"><MoneyInput value={task.deposit ?? 0} onCommit={(deposit) => set({ deposit }, "Edit deposit")} /></Field>
    </Optional>
    <Optional title="Classes" present={task.occupations !== null} onAdd={() => set({ occupations: [] }, "Add class requirement")} onRemove={() => set({ occupations: null }, "Remove class requirement")}>
      <IdList ids={task.occupations ?? []} max={LIMITS.occupations} placeholder="class ID" render={(id) => sets.task_occupation?.get(id) && <span className="dyn-label">{sets.task_occupation.get(id)}</span>} onCommit={(occupations) => set({ occupations }, "Edit classes")} />
    </Optional>
    <Optional title="Quests finished first" present={task.premiseTasks !== null} onAdd={() => set({ premiseTasks: [] }, "Add prerequisite quests")} onRemove={() => set({ premiseTasks: null }, "Remove prerequisite quests")}>
      <IdList ids={task.premiseTasks ?? []} max={LIMITS.premiseTasks} placeholder="task ID" render={taskLabel} onCommit={(premiseTasks) => set({ premiseTasks }, "Edit prerequisite quests")} />
    </Optional>
    <Optional title="Exclusive quests" present={task.mutexTasks !== null} onAdd={() => set({ mutexTasks: [] }, "Add exclusive quests")} onRemove={() => set({ mutexTasks: null }, "Remove exclusive quests")} hint="Cannot be taken while one of these is active">
      <IdList ids={task.mutexTasks ?? []} max={LIMITS.mutexTasks} placeholder="task ID" render={taskLabel} onCommit={(mutexTasks) => set({ mutexTasks }, "Edit exclusive quests")} />
    </Optional>
    <Optional title="Required items" present={task.premiseItems !== null} onAdd={() => set({ premiseItems: [] }, "Add required items")} onRemove={() => set({ premiseItems: null }, "Remove required items")}>
      <ItemTable items={task.premiseItems ?? []} max={LIMITS.list} labels={labels} onCommit={(premiseItems) => set({ premiseItems }, "Edit required items")} />
    </Optional>
    <Optional title="Given items" present={task.givenItems !== null} onAdd={() => set({ givenItems: { commonCount: 0, taskCount: 0, items: [] } }, "Add given items")} onRemove={() => set({ givenItems: null }, "Remove given items")} hint="Handed to the player when the task is accepted">
      <ItemTable items={task.givenItems?.items ?? []} max={LIMITS.list} labels={labels} onCommit={(items) => set({ givenItems: { commonCount: 0, taskCount: 0, items } }, "Edit given items")} />
    </Optional>
    <Optional title="Delivery zone" present={task.zone !== null} onAdd={() => set({ zone: { flag: 1, world: 1, min: origin(), max: origin() } }, "Add delivery zone")} onRemove={() => set({ zone: null }, "Remove delivery zone")} hint="Only given inside this box">
      {task.zone && <div className="dyn-grid">
        <Field label="World"><NumberInput value={task.zone.world} onCommit={(world) => set({ zone: { ...task.zone!, world } }, "Edit delivery zone")} /></Field>
        <Field label="From"><VertInput value={task.zone.min} onCommit={(min) => set({ zone: { ...task.zone!, min } }, "Edit delivery zone")} /></Field>
        <Field label="To"><VertInput value={task.zone.max} onCommit={(max) => set({ zone: { ...task.zone!, max } }, "Edit delivery zone")} /></Field>
      </div>}
    </Optional>
    <Optional title="Transfer" present={task.transfer !== null} onAdd={() => set({ transfer: { flag: 1, world: 1, point: origin() } }, "Add transfer")} onRemove={() => set({ transfer: null }, "Remove transfer")} hint="Moves the player when the task is accepted">
      {task.transfer && <div className="dyn-grid">
        <Field label="World"><NumberInput value={task.transfer.world} onCommit={(world) => set({ transfer: { ...task.transfer!, world } }, "Edit transfer")} /></Field>
        <Field label="Point"><VertInput value={task.transfer.point} onCommit={(point) => set({ transfer: { ...task.transfer!, point } }, "Edit transfer")} /></Field>
      </div>}
    </Optional>
    <Optional title="Timetable" present={task.timetable !== null} onAdd={() => set({ timetable: [] }, "Add timetable")} onRemove={() => set({ timetable: null }, "Remove timetable")} hint="When the task can be taken">
      <Timetable entries={task.timetable ?? []} types={sets.task_timetable_type} onCommit={(timetable) => set({ timetable }, "Edit timetable")} />
    </Optional>
  </div>;

  if (tab === "goal") {
    const goal = task.goal;
    const setGoal = (change: Partial<DynTask["goal"]>, label: string) => set({ goal: { ...goal, ...change } }, label);
    return <div className="dyn-form">
      <div className="dyn-grid"><Field label="Method"><Select value={goal.method} set={sets.task_method} fallback="Method" onCommit={(method) => setGoal({ method }, "Change method")} /></Field></div>
      {goal.method === 1 && <MonsterTable monsters={goal.monsters} labels={labels} onCommit={(monsters) => setGoal({ monsters }, "Edit monsters to kill")} />}
      {goal.method === 2 && <>
        <ItemTable items={goal.items} max={LIMITS.itemsWanted} labels={labels} onCommit={(items) => setGoal({ items }, "Edit items to collect")} />
        <div className="dyn-grid"><Field label="Gold to collect"><MoneyInput value={goal.gold} onCommit={(gold) => setGoal({ gold }, "Edit gold to collect")} /></Field></div>
      </>}
      {(goal.method === 4 || goal.method === 13) && <div className="dyn-grid">
        <Field label="Site"><NumberInput value={goal.siteId} onCommit={(siteId) => setGoal({ siteId }, "Edit site")} /></Field>
        <Field label="From"><VertInput value={goal.siteMin} onCommit={(siteMin) => setGoal({ siteMin }, "Edit site")} /></Field>
        <Field label="To"><VertInput value={goal.siteMax} onCommit={(siteMax) => setGoal({ siteMax }, "Edit site")} /></Field>
      </div>}
      {goal.method === 5 && <div className="dyn-grid"><Field label="Wait (seconds)" hint={goal.wait ? formatDuration(goal.wait) : undefined}><NumberInput value={goal.wait} onCommit={(wait) => setGoal({ wait }, "Edit wait time")} /></Field></div>}
      {!METHODS_WITH_DATA.has(goal.method) && <div className="empty-note">This method stores no goal data in dyn_tasks.data{goal.method === 3 ? ": the player completes the task by talking to the NPC (award talk)." : "."}</div>}
    </div>;
  }

  if (tab === "rewards") {
    const award = task.award;
    const setAward = (change: Partial<DynTask["award"]>, label: string) => set({ award: { ...award, ...change } }, label);
    const optional = (key: "sp" | "reputation" | "experience", title: string) => <Optional title={title} present={award[key] !== null} onAdd={() => setAward({ [key]: 0 }, `Add ${title.toLowerCase()} reward`)} onRemove={() => setAward({ [key]: null }, `Remove ${title.toLowerCase()} reward`)}>
      <Field label={title}><NumberInput min={key === "reputation" ? I32_MIN : 0} max={key === "reputation" ? I32_MAX : key === "experience" ? Number.MAX_SAFE_INTEGER : U32} value={award[key] ?? 0} onCommit={(value) => setAward({ [key]: value }, `Edit ${title.toLowerCase()} reward`)} /></Field>
    </Optional>;
    const candidates = award.candidates;
    const setCandidates = (next: DynCandidate[] | null, label: string) => setAward({ candidates: next }, label);
    return <div className="dyn-form">
      <Optional title="Gold" present={award.gold !== null} onAdd={() => setAward({ gold: 0 }, "Add gold reward")} onRemove={() => setAward({ gold: null }, "Remove gold reward")}>
        <Field label="Money"><MoneyInput value={award.gold ?? 0} onCommit={(gold) => setAward({ gold }, "Edit gold reward")} /></Field>
      </Optional>
      {optional("experience", "Experience")}
      {optional("sp", "SP")}
      {optional("reputation", "Reputation")}
      <Optional title="Item rewards" present={candidates !== null} hint={layout === "unknown" && candidates === null ? "This pack has no item rewards, so where they are stored is unknown" : "One group is given (or chosen)"} onAdd={() => layout === "unknown" ? undefined : setCandidates([{ random: 0, items: [] }], "Add item rewards")} onRemove={() => setCandidates(null, "Remove item rewards")}>
        {(candidates ?? []).map((candidate, index) => <div className="dyn-candidate" key={index}>
          <header><b>Group {index + 1}</b><label className="small"><input type="checkbox" checked={candidate.random !== 0} onChange={(event) => setCandidates(candidates!.map((entry, position) => position === index ? { ...entry, random: event.target.checked ? 1 : 0 } : entry), "Edit item rewards")} /> Random item</label><span className="spacer" />
            <button className="icon-btn small danger" onClick={() => setCandidates(candidates!.filter((_, position) => position !== index), "Remove reward group")} title="Remove this group"><Trash2 size={12} /></button></header>
          <ItemTable items={candidate.items} max={LIMITS.awardItems} labels={labels} onCommit={(items) => setCandidates(candidates!.map((entry, position) => position === index ? { ...entry, items } : entry), "Edit item rewards")} />
        </div>)}
        <button className="btn small" disabled={(candidates?.length ?? 0) >= LIMITS.candidates} onClick={() => setCandidates([...(candidates ?? []), { random: 0, items: [] }], "Add reward group")}><Plus size={12} /> Add group</button>
      </Optional>
    </div>;
  }

  if (tab === "texts") return <div className="dyn-form">
    <Field label="Description"><TextInput multiline value={task.description} onCommit={(description) => set({ description }, "Edit description")} /></Field>
    <Field label="Success text"><TextInput multiline value={task.okText} onCommit={(okText) => set({ okText }, "Edit success text")} /></Field>
    <Field label="Failure text"><TextInput multiline value={task.noText} onCommit={(noText) => set({ noText }, "Edit failure text")} /></Field>
  </div>;

  return <DialogTreeEditor dialogs={dialogs} taskId={task.id} taskName={task.name} renderParameter={(_, __, ___, option) => taskLabel(option.parameter)} onSave={onDialog} />;
}

function Timetable({ entries, types, onCommit }: { entries: DynTimetableEntry[]; types?: Map<number, string>; onCommit: (entries: DynTimetableEntry[]) => void }) {
  const set = (index: number, change: Partial<DynTimetableEntry>) => onCommit(entries.map((entry, position) => position === index ? { ...entry, ...change } : entry));
  return <div className="dyn-table-wrap">
    <table className="dyn-table">
      <thead><tr><th>Type</th><th>Start (year month day hour minute weekday)</th><th>End</th><th /></tr></thead>
      <tbody>{entries.map((entry, index) => <tr key={index}>
        <td><Select value={entry.kind} set={types} fallback="Type" onCommit={(kind) => set(index, { kind })} /></td>
        <td><TimeInput value={entry.start} onCommit={(start) => set(index, { start })} /></td>
        <td><TimeInput value={entry.end} onCommit={(end) => set(index, { end })} /></td>
        <td><button className="icon-btn small danger" onClick={() => onCommit(entries.filter((_, position) => position !== index))}><Trash2 size={12} /></button></td>
      </tr>)}</tbody>
    </table>
    <div className="dyn-table-foot"><button className="btn small" disabled={entries.length >= LIMITS.timetable} onClick={() => onCommit([...entries, { kind: 0, start: noTime(), end: noTime() }])}><Plus size={12} /> Add period</button><span className="muted small">{entries.length} / {LIMITS.timetable}</span></div>
  </div>;
}

// ── The workspace ──

const BACKUP_KEY = "jdide.dyn.backup";

export const DynTasksEditor = forwardRef<DynTasksEditorHandle, Props>(function DynTasksEditor({ active, onStateChange }, ref) {
  const [view, setView] = useState<DynView | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [path, setPath] = useState<number[]>([]);
  const [task, setTask] = useState<DynTask | null>(null);
  const [tab, setTab] = useState<Tab>("general");
  const [panel, setPanel] = useState<DynPanel>(null);
  const [problems, setProblems] = useState<DynProblemReport | null>(null);
  const [labels, setLabels] = useState<DynLabels>({ elements: {}, tasks: {} });
  const [sets, setSets] = useState<Record<string, Map<number, string>>>({});
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query.trim().toLowerCase());
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [saving, setSaving] = useState<{ target: string | null; changed: boolean } | null>(null);
  const [backup, setBackup] = useState(() => { try { return localStorage.getItem(BACKUP_KEY) !== "0"; } catch { return true; } });
  const savedOnce = useRef(false);
  const row = view && selected !== null ? view.rows[selected] ?? null : null;

  // The workspace remounts when switching; pick up the open pack.
  useEffect(() => {
    dynTasksView().then((current) => { if (current) { setView(current); setSelected(current.rows.length ? 0 : null); } }).catch(() => {});
  }, []);
  useEffect(() => {
    for (const key of ["task_dynamic_type", "task_finish_type", "task_method", "task_gender", "task_occupation", "task_timetable_type"]) {
      void loadSet(key).then((set) => setSets((current) => ({ ...current, [key]: set })));
    }
  }, []);

  useEffect(() => onStateChange({ loaded: !!view, dirty: !!view?.dirty, canUndo: !!view?.canUndo, canRedo: !!view?.canRedo, path: view?.path ?? null, tasks: view?.rows.length ?? 0, timeMark: view?.timeMark ?? null, panel }), [onStateChange, panel, view]);

  // Load the selected task (again after every change to the pack).
  useEffect(() => {
    if (!row) { setTask(null); return; }
    let cancelled = false;
    getDynTask(row.index, row.uid).then((next) => {
      if (cancelled) return;
      setTask(next);
      const ids = referencedIds(next);
      if (ids.elements.length || ids.tasks.length) dynTaskLabels(ids.elements, ids.tasks).then((found) => { if (!cancelled) setLabels((current) => ({ elements: { ...current.elements, ...found.elements }, tasks: { ...current.tasks, ...found.tasks } })); }).catch(() => {});
    }).catch((problem) => { if (!cancelled) setError(String(problem).replace(/^Error: /, "")); });
    return () => { cancelled = true; };
  }, [row?.index, row?.uid, view]);

  const run = useCallback(async (work: () => Promise<DynView | null | void>) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const next = await work();
      if (next) setView(next);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  }, [busy]);

  const load = useCallback(async (file: string) => {
    if (view?.dirty && !window.confirm("dyn_tasks.data has unsaved changes. Open another file and discard them?")) return;
    await run(async () => {
      const next = await openDynTasks(file);
      setSelected(next.rows.length ? 0 : null);
      setPath([]);
      setProblems(null);
      savedOnce.current = false;
      return next;
    });
  }, [run, view?.dirty]);

  const choose = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, defaultPath: view?.path, title: "Open dyn_tasks.data (the server's copy, e.g. gamed/config)", filters: [{ name: "dyn_tasks.data", extensions: ["data"] }] });
    if (typeof picked === "string") await load(picked);
  }, [load, view?.path]);

  const commit = useCallback((node: DynTask, label: string) => {
    if (!row || !task) return;
    const next = withNode(task, path, node);
    void run(() => setDynTask(row.index, row.uid, next, label));
  }, [path, row, run, task]);

  const saveDialog = useCallback(async (dialog: TaskDialog, label: string) => {
    if (!row || !task) return;
    const node = nodeAt(task, path);
    const index = TALK_KEYS.indexOf(dialog.talk);
    const talks = node.talks.map((talk, position) => position === index ? fromDialog(dialog) : talk);
    const next = await setDynTask(row.index, row.uid, withNode(task, path, { ...node, talks }), label);
    setView(next);
  }, [path, row, task]);

  const undo = useCallback(() => { if (view?.canUndo) void run(undoDynTask); }, [run, view?.canUndo]);
  const redo = useCallback(() => { if (view?.canRedo) void run(redoDynTask); }, [run, view?.canRedo]);
  const cloneSelected = useCallback(() => {
    if (!row) return;
    void run(async () => {
      const result = await cloneDynTask(row.index, row.uid);
      setSelected(result.index);
      setPath([]);
      setNote(`Cloned task ${row.id} as task ${result.view.rows[result.index].id}${row.dynType === 1 ? ` with special award ${result.view.rows[result.index].specialAward}` : ""}.`);
      return result.view;
    });
  }, [row, run]);
  const deleteSelected = useCallback(() => {
    if (!row || !view) return;
    if (!window.confirm(`Delete task ${row.id} "${row.name}"${row.subtasks ? ` and its ${row.subtasks} subtask${row.subtasks === 1 ? "" : "s"}` : ""}? Undo brings it back.`)) return;
    void run(async () => {
      const next = await deleteDynTask(row.index, row.uid);
      setSelected(next.rows.length ? Math.min(row.index, next.rows.length - 1) : null);
      setPath([]);
      return next;
    });
  }, [row, run, view]);

  const refreshProblems = useCallback(() => { void dynTaskProblems().then(setProblems).catch((problem) => setError(String(problem))); }, []);
  const toggleProblems = useCallback(() => setPanel((current) => { const next = current === "problems" ? null : "problems"; if (next) refreshProblems(); return next; }), [refreshProblems]);
  const toggleHistory = useCallback(() => setPanel((current) => current === "history" ? null : "history"), []);

  const writeTo = useCallback(async (target: string | null, replaceChanged: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const report = await saveDynTasks(target, backup, replaceChanged);
      savedOnce.current = true;
      setSaving(null);
      setNote(`Saved ${report.tasks} tasks (${bytes(report.size)}) to ${report.path} with time mark ${new Date(report.timeMark * 1000).toLocaleString()}.${report.backup ? ` Backup: ${report.backup}.` : ""} Clients download it from the server on their next login.`);
      setView(await dynTasksView());
    } catch (problem) {
      const message = String(problem).replace(/^Error: /, "");
      if (message.startsWith("CHANGED_ON_DISK")) setSaving({ target, changed: true });
      else setError(message);
    } finally {
      setBusy(false);
    }
  }, [backup]);
  const saveCurrent = useCallback(() => {
    if (!view) return;
    if (savedOnce.current) void writeTo(null, false);
    else setSaving({ target: null, changed: false });
  }, [view, writeTo]);
  const saveAs = useCallback(async () => {
    if (!view) return;
    const target = await save({ defaultPath: view.path, title: "Save dyn_tasks.data as", filters: [{ name: "dyn_tasks.data", extensions: ["data"] }] });
    if (target) setSaving({ target, changed: false });
  }, [view]);

  useImperativeHandle(ref, () => ({ choose: () => void choose(), openPath: (file) => void load(file), save: saveCurrent, saveAs: () => void saveAs(), undo, redo, cloneSelected, deleteSelected, toggleProblems, toggleHistory }), [choose, cloneSelected, deleteSelected, load, redo, saveAs, saveCurrent, toggleHistory, toggleProblems, undo]);

  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      const mod = event.ctrlKey || event.metaKey;
      const typing = event.target instanceof HTMLElement && /^(INPUT|TEXTAREA|SELECT)$/.test(event.target.tagName);
      const key = event.key.toLowerCase();
      if (mod && ["o", "s", "z", "y", "d", "h"].includes(key)) {
        // Undo inside a text box is the box's own.
        if (typing && (key === "z" || key === "y")) return;
        event.preventDefault();
        event.stopImmediatePropagation();
        if (key === "o") void choose();
        else if (key === "s") event.shiftKey ? void saveAs() : saveCurrent();
        else if (key === "z") event.shiftKey ? redo() : undo();
        else if (key === "y") redo();
        else if (key === "d") cloneSelected();
        else toggleHistory();
      } else if (mod && event.shiftKey && key === "m") {
        event.preventDefault();
        toggleProblems();
      } else if (event.key === "Delete" && !typing) {
        event.preventDefault();
        deleteSelected();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [active, choose, cloneSelected, deleteSelected, redo, saveAs, saveCurrent, toggleHistory, toggleProblems, undo]);

  const visible = useMemo(() => (view?.rows ?? []).filter((entry) => !deferredQuery || String(entry.id).includes(deferredQuery) || entry.name.toLowerCase().includes(deferredQuery) || String(entry.specialAward) === deferredQuery), [deferredQuery, view?.rows]);

  if (!view) return <section className="dyn-tasks-pane empty">
    <div className="drop-card tasks-empty">
      <Gift size={34} />
      <h2>Open a dyn_tasks.data file</h2>
      <p className="muted">Dynamic tasks are the gift quests the server hands out (special awards). Open the <b>server's</b> copy, for example <span className="mono">gamed/config/dyn_tasks.data</span>: clients keep their own copy in <span className="mono">userdata</span> and download the server's whenever its time mark changes.</p>
      <button className="btn primary" onClick={() => void choose()} disabled={busy}>{busy ? <Loader2 size={14} className="spin" /> : <FolderOpen size={14} />} Choose dyn_tasks.data…</button>
      {error && <div className="path-data-message error">{error}</div>}
    </div>
  </section>;

  const node = task ? nodeAt(task, path) : null;
  const tree = task ? nodes(task) : [];
  return <section className="dyn-tasks-pane" aria-busy={busy}>
    <header className="tasks-head">
      <div><h2>Dynamic tasks {view.dirty && <span className="tag warn">unsaved</span>}</h2><div className="tasks-file-line">
        <span className="mono truncate" title={view.path}>{view.path}</span>
        <span className="path-data-badge"><b>Tasks:</b> {count(view.rows.length)}</span>
        <span className="path-data-badge"><b>Size:</b> {bytes(view.size)}</span>
        <span className="path-data-badge" title="Clients replace their copy when this changes; saving sets it to now"><b>Time mark:</b> {new Date(view.timeMark * 1000).toLocaleString()}</span>
        <span className="path-data-badge" title="Where the award stores item rewards: bit 4 (older clients) or bit 5 (HDN, Reborn)"><b>Rewards:</b> {view.layout === "classic" ? "older layout" : view.layout === "shifted" ? "newer layout" : "not used"}</span>
      </div></div>
      <button className="btn" onClick={undo} disabled={!view.canUndo || busy} title="Undo (Ctrl+Z)"><Undo2 size={14} /></button>
      <button className="btn" onClick={redo} disabled={!view.canRedo || busy} title="Redo (Ctrl+Y)"><Redo2 size={14} /></button>
      <button className={"btn" + (panel === "problems" ? " active" : "")} onClick={toggleProblems} title="Problems (Ctrl+Shift+M)"><CircleAlert size={14} /> Problems</button>
      <button className={"btn" + (panel === "history" ? " active" : "")} onClick={toggleHistory} title="History (Ctrl+H)"><History size={14} /> History</button>
      <button className="btn" onClick={() => void choose()} disabled={busy}><FolderOpen size={14} /> Open…</button>
      <button className="btn primary" onClick={saveCurrent} disabled={busy}><Save size={14} /> Save</button>
    </header>
    {error && <div className="path-data-message error">{error} <button className="link" onClick={() => setError(null)}>Dismiss</button></div>}
    {note && <div className="path-data-message ok">{note} <button className="link" onClick={() => setNote(null)}>Dismiss</button></div>}
    <div className="dyn-tasks-body">
      <aside className="dyn-task-list">
        <div className="dyn-task-search"><Search size={13} /><input value={query} placeholder="ID, name or special award" onChange={(event) => setQuery(event.target.value)} />{query && <button className="icon-btn small" onClick={() => setQuery("")}><X size={12} /></button>}</div>
        <div className="dyn-task-rows" role="listbox">
          {visible.map((entry) => <button key={entry.uid} role="option" aria-selected={entry.index === selected} className={"dyn-task-row" + (entry.index === selected ? " selected" : "") + (entry.status ? ` ${entry.status}` : "")} onClick={() => { setSelected(entry.index); setPath([]); }}>
            <span className="mono">{entry.id}</span>
            <span className="truncate">{entry.name || <span className="muted">(no name)</span>}</span>
            {entry.dynType === 1 && <span className="dyn-award-badge" title="Special award number">#{entry.specialAward}</span>}
            {entry.status && <span className={"changed-dot" + (entry.status === "added" ? " added" : "")} title={entry.status} />}
          </button>)}
          {!visible.length && <div className="empty-note">No task matches.</div>}
        </div>
        <footer>
          <button className="btn small" onClick={cloneSelected} disabled={!row || busy} title="Copy with fresh IDs and the next special award number (Ctrl+D)"><Copy size={13} /> Clone</button>
          <button className="btn small danger" onClick={deleteSelected} disabled={!row || busy} title="Delete (Del)"><Trash2 size={13} /> Delete</button>
          <span className="spacer" />
          <span className="muted small">{count(visible.length)} / {count(view.rows.length)}</span>
        </footer>
      </aside>
      <div className="dyn-task-main">
        {panel === "problems" ? <section className="dyn-panel">
          <header><b>Problems</b><span className="muted small">{problems ? `${problems.problems.length} found${problems.tasksChecked ? "" : " · open tasks.data to check task IDs"}${problems.elementsChecked ? "" : " · open elements.data to check items"}` : "Checking…"}</span><span className="spacer" /><button className="btn small" onClick={refreshProblems}>Check again</button><button className="icon-btn small" onClick={() => setPanel(null)}><X size={13} /></button></header>
          {problems && (problems.problems.length ? problems.problems.map((problem, index) => <button key={index} className="dyn-problem" onClick={() => { setSelected(problem.index); setPath([]); setPanel(null); }}>
            <span className={"tag " + (problem.severity === "error" ? "danger" : "warn")}>{problem.severity}</span><span className="mono">{problem.taskId}</span><span className="truncate">{problem.taskName}</span><span>{problem.message}</span>
          </button>) : <div className="empty-note">No problems found.</div>)}
        </section> : panel === "history" ? <section className="dyn-panel">
          <header><b>History</b><span className="muted small">{view.history.length ? `${view.history.length} edit${view.history.length === 1 ? "" : "s"}` : "No edits yet"}</span><span className="spacer" /><button className="icon-btn small" onClick={() => setPanel(null)}><X size={13} /></button></header>
          {view.history.map((entry, index) => <div key={entry.id} className={"dyn-history-row" + (entry.undone ? " undone" : "")}>
            <span className="muted small">{new Date(entry.time * 1000).toLocaleTimeString()}</span><span>{entry.label}</span><span className="mono">{entry.taskId}</span><span className="truncate muted">{entry.taskName}</span>
            {view.savedEntries === index + 1 && !entry.undone && <span className="tag ok">Saved</span>}
          </div>)}
        </section> : !row || !task || !node ? <div className="empty-note">Select a task.</div> : <>
          <div className="dyn-task-title">
            <h3><span className="mono">{node.id}</span> {node.name}</h3>
            {tree.length > 1 && <select className="task-form-select" value={path.join(".")} onChange={(event) => setPath(event.target.value ? event.target.value.split(".").map(Number) : [])}>{tree.map((entry) => <option key={entry.path.join(".")} value={entry.path.join(".")}>{"  ".repeat(entry.depth)}{entry.task.id} · {entry.task.name}</option>)}</select>}
            {row.status && <span className={"tag " + (row.status === "added" ? "ok" : "warn")}>{row.status}</span>}
          </div>
          <div className="task-award-tabs" role="tablist">{TABS.map((entry) => <button key={entry.key} role="tab" aria-selected={tab === entry.key} className={tab === entry.key ? "active" : ""} onClick={() => setTab(entry.key)}>{entry.label}</button>)}</div>
          <div className="dyn-task-form-scroll">
            <DynTaskForm key={`${row.uid}:${path.join(".")}`} task={node} top={!path.length} tab={tab} layout={view.layout} labels={labels} sets={sets} onCommit={commit} onDialog={saveDialog} />
          </div>
        </>}
      </div>
    </div>
    {saving && <div className="modal-backdrop" onMouseDown={() => !busy && setSaving(null)}>
      <div className="modal dyn-save-dialog" onMouseDown={(event) => event.stopPropagation()}>
        <h3>Save dyn_tasks.data</h3>
        <p className="mono truncate" title={saving.target ?? view.path}>{saving.target ?? view.path}</p>
        <p className="muted small">Saving sets the time mark to now. The server sends the new pack to every client on their next login, so save the <b>server's</b> copy (for example <span className="mono">gamed/config/dyn_tasks.data</span>) and restart or reload the server.</p>
        {saving.changed && <div className="path-data-message error"><AlertTriangle size={14} /> Another program changed this file since it was opened. Replacing it discards those changes.</div>}
        <label className="small"><input type="checkbox" checked={backup} onChange={(event) => { setBackup(event.target.checked); try { localStorage.setItem(BACKUP_KEY, event.target.checked ? "1" : "0"); } catch { /* Remembering is optional. */ } }} /> Back up the existing file first (<span className="mono">.bak</span>, once per session)</label>
        <footer><span className="spacer" /><button className="btn" onClick={() => setSaving(null)} disabled={busy}>Cancel</button><button className="btn primary" onClick={() => void writeTo(saving.target, saving.changed)} disabled={busy}>{busy ? <Loader2 size={14} className="spin" /> : <Save size={14} />} {saving.changed ? "Replace anyway" : "Save"}</button></footer>
      </div>
    </div>}
  </section>;
});
