import { useEffect, useState, type ReactNode } from "react";
import { ArrowDown, ArrowUp, Copy, CornerLeftUp, Plus, Search, Trash2 } from "lucide-react";
import { NumberInput, TextInput } from "./DynTasksEditor";
import type { AiCatalog, AiCondition, AiConditionEdit, AiOperation, AiOperationEdit, AiParam, AiTrigger, AiTriggerEdit, AiValue } from "../elements/types";

export const ATTACK_TYPES = ["Melee", "Ranged (physical)", "Magic", "Melee and ranged"];
export const PATH_TYPES = ["Stop at the end", "Go back along the path", "Loop"];
/** Channels a talk text can start with (op_say). */
export const CHANNELS: Record<string, string> = { $B: "World broadcast", $A: "Broadcast (channel 9)", $F: "Battle faction channel", $T: "Battle channel" };

export function channelOf(text: string): [string | null, string] {
  const prefix = text.slice(0, 2);
  return CHANNELS[prefix] ? [prefix, text.slice(2)] : [null, text];
}

const U32 = 4294967295, I32_MIN = -2147483648, I32_MAX = 2147483647;
const NOT = 5, OR = 6, AND = 7;
const isLogic = (kind: number) => kind === NOT || kind === OR || kind === AND;
const isArithmetic = (kind: number) => kind >= 12 && kind <= 15;
const isComparison = (kind: number) => kind >= 16 && kind <= 18;
/** Values a new condition or operation starts with (zeros otherwise). */
const CONDITION_DEFAULTS: Record<number, Record<string, AiValue>> = { 1: { ratio: 0.5 }, 3: { chance: 0.5 }, 10: { players: 1 }, 11: { distance: 20 } };
const OPERATION_DEFAULTS: Record<number, Record<string, AiValue>> = { 1: { level: 1 }, 7: { period: 10, times: 1 }, 15: { count: 1 }, 20: { level: 1 }, 25: { count: 1 }, 26: { count: 1 }, 30: { count: 1 } };
/** A part that always holds (Random 100%), for new And/Or parts. */
const ALWAYS: AiConditionEdit = { kind: 3, values: { chance: 1 }, raw: [] };
const NUMBER: AiConditionEdit = { kind: 20, values: { value: 0 }, raw: [] };
const VARIABLE: AiConditionEdit = { kind: 19, values: { variable: 0 }, raw: [] };

const valuesOf = (params: AiParam[]) => Object.fromEntries(params.map((param) => [param.key, param.value]));

export function conditionEdit(node: AiCondition): AiConditionEdit {
  return { kind: node.kind, values: valuesOf(node.params), raw: node.raw, left: node.left ? conditionEdit(node.left) : null, right: node.right ? conditionEdit(node.right) : null };
}

export function operationEdit(operation: AiOperation): AiOperationEdit {
  return { kind: operation.kind, values: valuesOf(operation.params), raw: operation.raw, target: operation.target, targetMask: operation.targetMask };
}

export function triggerEdit(trigger: AiTrigger): AiTriggerEdit {
  return { name: trigger.name, active: trigger.active, run: trigger.run, battleOnly: trigger.battleOnly, condition: conditionEdit(trigger.condition), operations: trigger.operations.map(operationEdit) };
}

type Side = "left" | "right";

function nodeAt(node: AiConditionEdit, path: Side[]): AiConditionEdit {
  return path.reduce((current, side) => current[side]!, node);
}

function replaceAt(node: AiConditionEdit, path: Side[], next: AiConditionEdit): AiConditionEdit {
  if (!path.length) return next;
  const [side, ...rest] = path;
  return { ...node, [side]: replaceAt(node[side]!, rest, next) };
}

/** A node changed to another type: And/Or/Not wrap what was there, comparisons and arithmetic get values to compare. */
function retype(old: AiConditionEdit, kind: number): AiConditionEdit {
  const base: AiConditionEdit = { kind, values: { ...(CONDITION_DEFAULTS[kind] ?? {}) }, raw: [], left: null, right: null };
  if (kind === AND || kind === OR) {
    if (old.kind === AND || old.kind === OR) return { ...base, left: old.left, right: old.right };
    return { ...base, left: old, right: ALWAYS };
  }
  if (kind === NOT) return { ...base, right: old };
  if (isArithmetic(kind)) return isArithmetic(old.kind) ? { ...base, left: old.left, right: old.right } : { ...base, left: old, right: NUMBER };
  if (isComparison(kind)) return isComparison(old.kind) ? { ...base, left: old.left, right: old.right } : { ...base, left: VARIABLE, right: NUMBER };
  return base;
}

/** What the form needs from the editor: names, pickers and the policy's triggers. */
export interface FormContext {
  catalog: AiCatalog;
  triggers: AiTrigger[];
  busy: boolean;
  /** The name of what an ID refers to, if known. */
  nameOf: (param: AiParam) => ReactNode;
  /** Opens a picker for a skill, monster, item or mine; `apply` receives the chosen ID. */
  pick: (refers: string, current: number, apply: (value: number) => void) => void;
}

const PICKABLE = new Set(["skill", "monster", "item", "mine"]);

function PercentInput({ value, onCommit }: { value: number; onCommit: (value: number) => void }) {
  const shown = String(Math.round(value * 100000) / 1000);
  const [draft, setDraft] = useState(shown);
  useEffect(() => setDraft(shown), [shown]);
  const commit = () => {
    const number = Number(draft.trim().replace(/%$/, ""));
    if (!draft.trim() || !Number.isFinite(number)) { setDraft(shown); return; }
    if (String(number) !== shown) onCommit(number / 100);
  };
  return <span className="ai-percent"><input className="dyn-input mono" value={draft} onChange={(event) => setDraft(event.target.value)} onBlur={commit} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); if (event.key === "Escape") setDraft(shown); }} />%</span>;
}

/** A hex or decimal mask (classes of target "Classes"). */
function MaskInput({ value, onCommit }: { value: number; onCommit: (value: number) => void }) {
  const shown = "0x" + value.toString(16).toUpperCase();
  const [draft, setDraft] = useState(shown);
  useEffect(() => setDraft(shown), [shown]);
  const commit = () => {
    const text = draft.trim();
    const number = /^0x[0-9a-f]+$/i.test(text) ? parseInt(text.slice(2), 16) : /^\d+$/.test(text) ? Number(text) : NaN;
    if (!Number.isFinite(number) || number > U32) { setDraft(shown); return; }
    if (number !== value) onCommit(number);
  };
  return <input className="dyn-input mono ai-mask" value={draft} title="Class bit mask (hex with 0x, or decimal)" onChange={(event) => setDraft(event.target.value)} onBlur={commit} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); if (event.key === "Escape") setDraft(shown); }} />;
}

function NamedSelect({ value, options, onCommit }: { value: number; options: string[]; onCommit: (value: number) => void }) {
  return <select className="task-form-select" value={value} onChange={(event) => onCommit(Number(event.target.value))}>
    {options.map((label, index) => <option key={index} value={index}>{label}</option>)}
    {(value < 0 || value >= options.length) && <option value={value}>{value} (unknown)</option>}
  </select>;
}

function ParamInput({ param, ctx, onCommit }: { param: AiParam; ctx: FormContext; onCommit: (value: AiValue) => void }) {
  const value = param.value;
  if (param.kind === "bool") return <input type="checkbox" checked={!!value} disabled={ctx.busy} onChange={(event) => onCommit(event.target.checked)} />;
  if (param.kind === "name16") return <TextInput value={String(value)} max={16} placeholder="(the monster's own name)" onCommit={onCommit} />;
  if (typeof value !== "number") return <span className="muted">{String(value)}</span>;
  if (param.kind === "f32") return param.key === "ratio" || param.key === "chance" ? <PercentInput value={value} onCommit={onCommit} /> : <NumberInput value={value} float min={-1e9} max={1e9} onCommit={onCommit} />;
  if (param.refers === "attack") return <NamedSelect value={value} options={ATTACK_TYPES} onCommit={onCommit} />;
  if (param.refers === "path_type") return <NamedSelect value={value} options={PATH_TYPES} onCommit={onCommit} />;
  if (param.refers === "trigger") {
    const missing = !ctx.triggers.some((entry) => entry.id === value);
    return <select className="task-form-select ai-trigger-select" value={value} onChange={(event) => onCommit(Number(event.target.value))}>
      {missing && <option value={value}>{value} (not in this policy)</option>}
      {ctx.triggers.map((entry) => <option key={entry.index} value={entry.id}>{entry.id}{entry.name ? ` · ${entry.name}` : ""}{entry.run ? " (sub)" : ""}</option>)}
    </select>;
  }
  const [min, max] = param.kind === "i32" ? [I32_MIN, I32_MAX] : [0, U32];
  const name = ctx.nameOf(param);
  return <span className="ai-param-input">
    <NumberInput value={value} min={min} max={max} onCommit={onCommit} />
    {param.refers && PICKABLE.has(param.refers) && <button className="icon-btn small" title={`Choose a ${param.refers}`} disabled={ctx.busy} onClick={() => ctx.pick(param.refers!, value, onCommit)}><Search size={12} /></button>}
    {name && <span className="ai-param-name">{name}</span>}
  </span>;
}

function Params({ params, ctx, onCommit }: { params: AiParam[]; ctx: FormContext; onCommit: (key: string, value: AiValue) => void }) {
  if (!params.length) return null;
  return <span className="ai-edit-params">{params.map((param) => <label key={param.key} className="ai-edit-param"><span className="muted small">{param.label}</span><ParamInput param={param} ctx={ctx} onCommit={(value) => onCommit(param.key, value)} /></label>)}</span>;
}

interface Props {
  trigger: AiTrigger;
  ctx: FormContext;
  /** Applies a whole trigger (one undo step). */
  onApply: (edit: AiTriggerEdit) => void;
}

/** The editable trigger: name and flags, the condition tree and the operations. */
export function AiTriggerForm({ trigger, ctx, onApply }: Props) {
  const edit = triggerEdit(trigger);
  const apply = (change: Partial<AiTriggerEdit>) => onApply({ ...edit, ...change });
  const setCondition = (path: Side[], node: AiConditionEdit) => apply({ condition: replaceAt(edit.condition, path, node) });
  const setOperations = (operations: AiOperationEdit[]) => apply({ operations });
  const [adding, setAdding] = useState("");

  const conditionNode = (view: AiCondition | null | undefined, path: Side[], expression: boolean, parentKind: number | null): ReactNode => {
    const key = path.join("") || "root";
    if (!view) return <li key={key} className="ai-warn">(missing part)</li>;
    const node = nodeAt(edit.condition, path);
    const choices = ctx.catalog.conditions.filter((info) => info.expression === expression);
    return <li key={key} className="ai-edit-cond">
      <div className="ai-edit-cond-row">
        {path.length > 0 && <span className="muted small ai-side">{parentKind === NOT ? "" : path[path.length - 1] === "left" ? "1." : "2."}</span>}
        <select className="task-form-select" value={view.kind} disabled={ctx.busy} onChange={(event) => setCondition(path, retype(node, Number(event.target.value)))}>
          {choices.map((info) => <option key={info.kind} value={info.kind}>{info.name}{info.newer ? " (newer servers)" : ""}</option>)}
          {!choices.some((info) => info.kind === view.kind) && <option value={view.kind}>{view.name} ({view.kind})</option>}
        </select>
        <Params params={view.params} ctx={ctx} onCommit={(param, value) => setCondition(path, { ...node, values: { ...node.values, [param]: value } })} />
        {parentKind !== null && isLogic(parentKind) && <button className="icon-btn small" disabled={ctx.busy} title={parentKind === NOT ? "Remove the Not above (keep this part)" : "Replace the And/Or above with this part (drop the other part)"} onClick={() => setCondition(path.slice(0, -1), node)}><CornerLeftUp size={12} /></button>}
      </div>
      {(view.left || view.right || isLogic(view.kind) || isArithmetic(view.kind) || isComparison(view.kind)) && <ul className="ai-cond-children">
        {view.kind !== NOT && conditionNode(view.left, [...path, "left"], isArithmetic(view.kind) || isComparison(view.kind), view.kind)}
        {conditionNode(view.right, [...path, "right"], isArithmetic(view.kind) || isComparison(view.kind), view.kind)}
      </ul>}
    </li>;
  };

  const operationRow = (operation: AiOperation, index: number) => {
    const current = edit.operations[index];
    const replace = (next: AiOperationEdit) => setOperations(edit.operations.map((entry, at) => at === index ? next : entry));
    const move = (to: number) => {
      const list = [...edit.operations];
      const [moved] = list.splice(index, 1);
      list.splice(to, 0, moved);
      setOperations(list);
    };
    const text = operation.params.find((param) => param.kind === "text");
    const [channel, body] = typeof text?.value === "string" ? channelOf(text.value) : [null, ""];
    const setText = (prefix: string | null, rest: string) => replace({ ...current, values: { ...current.values, text: (prefix ?? "") + rest } });
    const choices = ctx.catalog.operations;
    return <li key={index} className="ai-op ai-edit-op">
      <span className="ai-op-index mono muted">{index + 1}</span>
      <div className="ai-op-body">
        <div className="ai-op-head">
          <select className="task-form-select" value={operation.kind} disabled={ctx.busy} onChange={(event) => { const kind = Number(event.target.value); replace({ kind, values: { ...(OPERATION_DEFAULTS[kind] ?? {}) }, raw: [], target: current.target, targetMask: current.targetMask }); }}>
            {choices.map((info) => <option key={info.kind} value={info.kind}>{info.name}{info.newer ? " (newer servers)" : ""}</option>)}
            {!choices.some((info) => info.kind === operation.kind) && <option value={operation.kind}>{operation.name} ({operation.kind})</option>}
          </select>
          {operation.usesTarget && <label className="ai-edit-param"><span className="muted small">target</span>
            <select className="task-form-select" value={operation.target} disabled={ctx.busy} onChange={(event) => { const target = Number(event.target.value); replace({ ...current, target, targetMask: target === 6 ? current.targetMask ?? 0 : null }); }}>
              {ctx.catalog.targets.map(([kind, name]) => <option key={kind} value={kind}>{name}</option>)}
              {!ctx.catalog.targets.some(([kind]) => kind === operation.target) && <option value={operation.target}>{operation.targetName} ({operation.target})</option>}
            </select>
            {operation.target === 6 && <MaskInput value={operation.targetMask ?? 0} onCommit={(targetMask) => replace({ ...current, targetMask })} />}
          </label>}
          <span className="spacer" />
          <button className="icon-btn small" disabled={ctx.busy || index === 0} title="Move up" onClick={() => move(index - 1)}><ArrowUp size={12} /></button>
          <button className="icon-btn small" disabled={ctx.busy || index + 1 >= edit.operations.length} title="Move down" onClick={() => move(index + 1)}><ArrowDown size={12} /></button>
          <button className="icon-btn small" disabled={ctx.busy} title="Copy below" onClick={() => setOperations([...edit.operations.slice(0, index + 1), current, ...edit.operations.slice(index + 1)])}><Copy size={12} /></button>
          <button className="icon-btn small danger" disabled={ctx.busy} title="Remove" onClick={() => setOperations(edit.operations.filter((_, at) => at !== index))}><Trash2 size={12} /></button>
        </div>
        {text ? <div className="ai-edit-talk">
          <select className="task-form-select" value={channel ?? ""} disabled={ctx.busy} title="Where the text appears" onChange={(event) => setText(event.target.value || null, body)}>
            <option value="">Nearby (speech bubble)</option>
            {Object.entries(CHANNELS).map(([prefix, name]) => <option key={prefix} value={prefix}>{name} ({prefix})</option>)}
          </select>
          <TextInput value={body} multiline placeholder="What it says" onCommit={(next) => setText(channel, next)} />
        </div> : <Params params={operation.params} ctx={ctx} onCommit={(param, value) => replace({ ...current, values: { ...current.values, [param]: value } })} />}
      </div>
    </li>;
  };

  return <div className="ai-trigger-form">
    <div className="ai-edit-head">
      <label className="ai-edit-param ai-edit-name"><span className="muted small">Name</span><TextInput value={trigger.name} placeholder="(no name)" onCommit={(name) => apply({ name })} /></label>
      <label className="check" title="Whether it is enabled when the monster spawns (Enable/Disable a trigger change it)"><input type="checkbox" checked={trigger.active} disabled={ctx.busy || trigger.run} onChange={(event) => apply({ active: event.target.checked })} /> Enabled at start</label>
      <label className="check" title="Not tested on its own: only another trigger's 'Run a trigger' runs it"><input type="checkbox" checked={trigger.run} disabled={ctx.busy} onChange={(event) => apply({ run: event.target.checked })} /> Sub-trigger</label>
      <label className="check" title="Tested only while the monster is in combat"><input type="checkbox" checked={trigger.battleOnly} disabled={ctx.busy} onChange={(event) => apply({ battleOnly: event.target.checked })} /> In combat only</label>
    </div>
    <h4 className="ai-section">When</h4>
    <p className="ai-expression mono">{trigger.expression}</p>
    <ul className="ai-cond ai-edit-tree">{conditionNode(trigger.condition, [], false, null)}</ul>
    <h4 className="ai-section">Then {trigger.operations.length > 1 && !trigger.selectOne && <span className="muted small">in order; "Skip the remaining operations" ends the list</span>}{trigger.selectOne && <span className="muted small">one of these runs at random</span>}</h4>
    {trigger.operations.length ? <ol className="ai-ops">{trigger.operations.map(operationRow)}</ol> : <p className="muted small">No operations.</p>}
    <div className="ai-add-op">
      <select className="task-form-select" value={adding} disabled={ctx.busy} onChange={(event) => setAdding(event.target.value)}>
        <option value="">Add an operation…</option>
        {ctx.catalog.operations.map((info) => <option key={info.kind} value={info.kind}>{info.name}{info.newer ? " (newer servers)" : ""}</option>)}
      </select>
      <button className="btn small" disabled={ctx.busy || adding === ""} onClick={() => { const kind = Number(adding); setOperations([...edit.operations, { kind, values: { ...(OPERATION_DEFAULTS[kind] ?? {}) }, raw: [], target: 0, targetMask: null }]); setAdding(""); }}><Plus size={12} /> Add</button>
    </div>
  </div>;
}
