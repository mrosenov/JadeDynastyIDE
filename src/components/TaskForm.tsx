import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { Copy, Plus, Trash2 } from "lucide-react";
import { characterClasses, namedSet } from "../elements/api";
import type { TaskArrayEdit, TaskDetail, TaskFieldReference, TaskFieldView } from "../elements/types";

/** The quest form's tabs; "advanced" is the raw field tree rendered by the editor. */
export type TaskFormTab = "general" | "locations" | "requirements" | "objectives" | "rewards" | "texts" | "advanced";

export const TASK_FORM_TABS: { key: TaskFormTab; label: string }[] = [
  { key: "general", label: "General" },
  { key: "locations", label: "Locations" },
  { key: "requirements", label: "Requirements" },
  { key: "objectives", label: "Objectives" },
  { key: "rewards", label: "Rewards" },
  { key: "texts", label: "Texts & dialogs" },
  { key: "advanced", label: "Advanced" },
];

export interface BatchValue {
  fieldPath: string[];
  value: string;
}

interface Props {
  detail: TaskDetail;
  tab: Exclude<TaskFormTab, "advanced">;
  onEdit: (field: TaskFieldView, value: string) => Promise<void>;
  /** Several values of the shown task as one undo step. */
  onBatch: (values: BatchValue[], label: string) => Promise<void>;
  /** Adds, clones or removes a row of a list; `countPath` names the count of a fixed list. */
  onRows: (field: TaskFieldView, countPath: string[] | null, change: TaskArrayEdit) => Promise<void>;
  renderReference: (reference: TaskFieldReference) => ReactNode;
  /** Opens Enums & masks at a set; dropdowns offer "Edit names…". */
  onEditSet?: (key: string) => void;
  /** Changes when enums or masks were edited, so the names are loaded again. */
  setsVersion?: number;
  /** Shown at the end of the General tab. */
  referencedBy?: ReactNode;
}

// ---------------------------------------------------------------- names and sets

/** Fields whose values have names, by the named set that holds them (editable in Enums & masks). */
const ENUM_FIELDS: Record<string, string> = {
  method: "task_method",
  finish_type: "task_finish_type",
  award_type_s: "task_award_type",
  award_type_f: "task_award_type",
  task_type: "task_type",
  display_type: "task_display_type",
  dynamic_task_type: "task_dynamic_type",
  avail_frequency: "task_avail_frequency",
  clear_receiver_type: "task_clear_receiver_type",
  gender: "task_gender",
  cotask_condition: "task_cotask_condition",
};
/** Fields whose bits have names (masks), shown as a row of checkboxes. */
const MASK_FIELDS: Record<string, string> = { recommend_type: "task_recommend_type" };
const SET_KEYS = [...new Set([...Object.values(ENUM_FIELDS), ...Object.values(MASK_FIELDS), "task_occupation", "task_friendship"])];
const setCache = new Map<string, Promise<Map<number, string>>>();
/** A named set's names by value (enums) or by bit (masks); cached until `setsVersion` changes. */
export function loadSet(key: string) {
  if (!setCache.has(key)) {
    // Enums map values to names, masks map bit numbers to names.
    setCache.set(key, namedSet(key).then((detail) => new Map(detail.set.flags ? detail.set.flags.map((flag) => [Number(flag.bit), flag.label] as [number, string]) : (detail.set.values ?? []).map((entry) => [Number(entry.value), entry.label] as [number, string]))).catch(() => {
      // Not cached, so a later form loads it again.
      setCache.delete(key);
      return new Map<number, string>();
    }));
  }
  return setCache.get(key)!;
}

interface FormContext {
  onEdit: Props["onEdit"];
  onBatch: Props["onBatch"];
  onRows: Props["onRows"];
  onEditSet?: Props["onEditSet"];
  renderReference: Props["renderReference"];
  sets: Record<string, Map<number, string>>;
  /** Classes of the open elements.data (CHARACTER_CLASS_CONFIG). */
  classes: [number, string][];
}
const Form = createContext<FormContext>({ onEdit: async () => {}, onBatch: async () => {}, onRows: async () => {}, renderReference: () => null, sets: {}, classes: [] });

const LABELS: Record<string, string> = {
  id: "ID", task_type: "Type", rand_one: "Random one", execute_child_in_order: "Subquests in order", parent_also_succ: "Parent also succeeds",
  avail_frequency: "Available frequency", time_interval: "Interval (seconds)", fail_as_player_die: "Fail on death", death_trig: "Death trigger",
  manual_trig: "Manual trigger", trans_to: "Transfer", trans_world_id: "Transfer world", trans_point: "Transfer point", hierarchy_parent: "Parent",
  hierarchy_previous_sibling: "Previous", hierarchy_next_sibling: "Next", hierarchy_first_child: "First subquest", award_type_s: "Award type (success)",
  award_type_f: "Award type (failure)", rec_finish_count: "Record finish count", rec_finish_count_global: "Record finish count globally",
  sj_deposit_battle_score: "Deposit SJ battle score", premise_sj_battle_score: "SJ battle score", premise_gm: "GM only", pk_value_min: "PK value min",
  pk_value_max: "PK value max", kermis: "Kermis", trig_control: "Trigger control", premise_task_count: "Quests finished first",
  premise_finish_task_count: "Quests with finish counts", mutex_task_count: "Exclusive quests", premise_global_task: "Global finish-count quest",
  premise_global_count: "Global finish count", premise_cotask: "Co-task", show_by_premise_task: "Show by finished quests",
  master_apprentice_tasks: "Master/apprentice quests", master_apprentice_task: "Master/apprentice quest", spirit: "SP", gold: "Gold",
  experience: "Exp", experience_coefficient: "Exp coefficient", experience_coefficient_2: "Exp coefficient 2", experience_coefficient_3: "Exp coefficient 3",
  fury_limit: "Fury upper limit", new_task_id: "Next quest", premise_items_not_taken: "Items not taken", show_by_items: "Show by items",
  occupation_count: "Classes", recommend_type: "Recommended for", display_type: "Quest log category", rank: "Difficulty (1–5 stars)",
  dynamic_task_type: "Dynamic type", clear_receiver_type: "Receiver limit reset", cotask_condition: "Co-task condition", show_by_occupation: "Show by class", common_item: "Common", bound: "Bind", random_choice: "Random choice",
  replacement_item_id: "Replace item ID", day_of_week: "Day of week", refine_condition: "Refine condition", refine_level: "Refine level",
};
const ACRONYMS: Record<string, string> = { npc: "NPC", id: "ID", pk: "PK", gm: "GM", xp: "XP", cd: "CD", sj: "SJ", pos: "position" };

export function fieldLabel(name: string) {
  if (LABELS[name]) return LABELS[name];
  const words = name.replace(/^premise_/, "").split("_").map((word) => ACRONYMS[word] ?? word);
  const text = words.join(" ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

const dotted = (path: string[]) => path.reduce((text, part) => part.startsWith("[") || !text ? text + part : `${text}.${part}`, "");
const isArray = (field: TaskFieldView) => !!field.children?.length && field.children[0].name.startsWith("[");
const childNames = (field: TaskFieldView) => field.children?.map((entry) => entry.name).join(",") ?? "";
const isTime = (field: TaskFieldView) => childNames(field) === "year,month,day,hour,minute,weekday";
const isVertex = (field: TaskFieldView) => childNames(field) === "x,y,z";
const isComposite = (field: TaskFieldView) => !!field.children?.length && !isTime(field) && !isVertex(field);
/** Bookkeeping the form leaves to the Advanced tab: pointers, raw bytes and vector headers. */
const hidden = (field: TaskFieldView) => field.name.endsWith("_pointer") || field.raw || /_(capacity|count)$/.test(field.name) && /^(change_|left_|right_)/.test(field.name);
const child = (field: TaskFieldView | undefined, name: string) => field?.children?.find((candidate) => candidate.name === name);

/** A value that means "not used": zero, false, empty text or zero bytes, throughout a structure. */
export function isDefault(field: TaskFieldView): boolean {
  if (field.children?.length) return field.children.every(isDefault);
  if (/^0 (items|fields)$/.test(field.value ?? "")) return true;
  if (/(^|_)transform/.test(field.name) && field.value === "-1") return true;
  const value = (field.value ?? "").replace(/\0+$/, "");
  return value === "" || value === "0" || value === "false" || /^(00 ?)+$/.test(value);
}

function index(fields: TaskFieldView[], into = new Map<string, TaskFieldView>()) {
  for (const field of fields) {
    into.set(dotted(field.path), field);
    if (field.children) index(field.children, into);
  }
  return into;
}

// ---------------------------------------------------------------- values

/** One editable value: a checkbox for flags, a dropdown for named values, otherwise a box that edits on click. */
function Value({ field, wide }: { field: TaskFieldView; wide?: boolean }) {
  const { onEdit, onEditSet, renderReference, sets } = useContext(Form);
  const [draft, setDraft] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const commit = async (value: string) => {
    setSaving(true);
    setError(null);
    try {
      await onEdit(field, value);
      setDraft(null);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setSaving(false);
    }
  };
  if (field.children?.length) {
    if (isTime(field) || isVertex(field)) {
      return <span className={"task-form-parts" + (isTime(field) ? " time" : "")}>{field.children.map((part) => <label key={part.name}><span>{isTime(field) ? part.name.slice(0, part.name === "minute" ? 3 : 1) : part.name}</span><Value field={part} /></label>)}</span>;
    }
    return <span className="muted">{field.value}</span>;
  }
  if (field.ty === "bool8") {
    return <input type="checkbox" className={field.changed ? "changed" : undefined} checked={field.value === "true"} disabled={!field.editable || saving} title={error ?? (field.editable ? undefined : "Locked: it controls the binary structure")} onChange={(event) => void commit(event.target.checked ? "true" : "false")} />;
  }
  const bits = MASK_FIELDS[field.name] ? sets[MASK_FIELDS[field.name]] : undefined;
  if (bits) {
    const current = Number(field.value ?? 0) >>> 0;
    const named = [...bits.entries()].sort((left, right) => left[0] - right[0]);
    const unnamed = Array.from({ length: 32 }, (_, bit) => bit).filter((bit) => current & (1 << bit) && !bits.has(bit));
    const toggle = (bit: number, on: boolean) => void commit(String((on ? current | (1 << bit) : current & ~(1 << bit)) >>> 0));
    return <span className={"task-form-mask" + (field.changed ? " changed" : "")} title={`Value ${current}`}>
      {[...named, ...unnamed.map((bit) => [bit, `Bit ${bit}`] as [number, string])].map(([bit, label]) => <label key={bit} className={current & (1 << bit) ? "checked" : undefined}>
        <input type="checkbox" checked={!!(current & (1 << bit))} disabled={!field.editable || saving} onChange={(event) => toggle(bit, event.target.checked)} />{label}
      </label>)}
      {onEditSet && <button className="link" onClick={() => onEditSet(MASK_FIELDS[field.name])}>Edit names…</button>}
      {error && <span className="task-field-edit-error">{error}</span>}
    </span>;
  }
  const names = ENUM_FIELDS[field.name] ? sets[ENUM_FIELDS[field.name]] : undefined;
  if (names) {
    const current = Number(field.value);
    const options = [...names.entries()];
    if (!names.has(current)) options.push([current, names.size ? "(no name)" : "(names not loaded)"]);
    return <span className={"task-form-value" + (field.changed ? " changed" : "")}>
      <select className="task-form-select" value={current} disabled={!field.editable || saving} onChange={(event) => { if (event.target.value === "__edit__") onEditSet?.(ENUM_FIELDS[field.name]); else void commit(event.target.value); }}>
        {options.sort((left, right) => left[0] - right[0]).map(([value, label]) => <option key={value} value={value}>[{value}] {label}</option>)}
        {onEditSet && <option value="__edit__">Edit names…</option>}
      </select>
      {error && <span className="task-field-edit-error">{error}</span>}
    </span>;
  }
  const text = field.ty.includes("wstring");
  const long = text && (wide || (field.value?.length ?? 0) > 60 || (field.value ?? "").includes("\n"));
  return <span className={"task-form-value" + (field.changed ? " changed" : "") + (long ? " long" : "") + (draft !== null || error ? " editing" : "")}>
    {draft !== null ? <span className="task-form-editor">
      {long ? <textarea value={draft} rows={4} autoFocus onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Escape") setDraft(null); if (event.key === "Enter" && event.ctrlKey) void commit(draft); }} />
        : <input value={draft} autoFocus spellCheck={text} onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void commit(draft); if (event.key === "Escape") setDraft(null); }} />}
      <button className="btn small" onClick={() => void commit(draft)} disabled={saving}>Apply</button>
      <button className="btn small" onClick={() => { setDraft(null); setError(null); }} disabled={saving}>Cancel</button>
    </span> : <button className={"task-form-box" + (text ? " text" : " mono")} disabled={!field.editable} onClick={() => setDraft(field.value ?? "")} title={field.editable ? (field.changed ? "Changed · click to edit" : "Click to edit") : "Locked: it controls the binary structure or the task tree"}>{field.value === "" ? <span className="muted">(empty)</span> : field.value}</button>}
    {field.reference && draft === null && renderReference(field.reference)}
    {error && <span className="task-field-edit-error">{error}</span>}
  </span>;
}

/** Labelled values in a compact multi-column grid; flags are checkboxes, text spans the full width. */
function Grid({ fields }: { fields: TaskFieldView[] }) {
  const shown = fields.filter((field) => !hidden(field));
  if (!shown.length) return null;
  return <div className="task-form-grid">{shown.map((field) => {
    const longText = field.ty.includes("wstring") && field.name !== "name" && field.name !== "signature";
    if (field.ty === "bool8") return <label key={dotted(field.path)} className={"task-form-check" + (field.changed ? " changed" : "")} title={dotted(field.path)}><Value field={field} />{fieldLabel(field.name)}</label>;
    return <div key={dotted(field.path)} className={"task-form-cell" + (longText || isTime(field) || isVertex(field) || MASK_FIELDS[field.name] ? " wide" : "")}><span className="task-form-label" title={dotted(field.path)}>{fieldLabel(field.name)}</span><Value field={field} wide={longText} /></div>;
  })}</div>;
}

/** A value in a table or list: an input that applies on Enter or when it loses focus (Escape restores it). */
function Cell({ field }: { field: TaskFieldView }) {
  const { onEdit, renderReference } = useContext(Form);
  const [draft, setDraft] = useState(field.value ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setDraft(field.value ?? ""), [field.value]);
  if (field.children?.length || field.ty === "bool8" || ENUM_FIELDS[field.name]) return <Value field={field} />;
  const commit = async () => {
    if (draft === (field.value ?? "")) {
      setError(null);
      return;
    }
    setSaving(true);
    setError(null);
    try {
      await onEdit(field, draft);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setSaving(false);
    }
  };
  const text = field.ty.includes("wstring");
  return <span className={"task-form-cell-value" + (field.changed ? " changed" : "")}>
    <input className={"task-form-input" + (text ? "" : " mono") + (error ? " invalid" : "")} value={draft} size={text ? 22 : Math.max(5, Math.min(14, draft.length + 2))} spellCheck={text}
      disabled={!field.editable || saving} title={error ?? (field.editable ? `${dotted(field.path)} · Enter applies, Escape restores` : "Locked: it controls the binary structure or the task tree")}
      onChange={(event) => setDraft(event.target.value)} onBlur={() => void commit()}
      onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); if (event.key === "Escape") { setDraft(field.value ?? ""); setError(null); } }} />
    {field.reference && renderReference(field.reference)}
    {error && <span className="task-field-edit-error">{error}</span>}
  </span>;
}

/** Add, clone and remove for a list's rows. Variable-length lists change size; fixed lists with a count fill their slots in order. */
function useRows(field: TaskFieldView, countField?: TaskFieldView) {
  const { onRows } = useContext(Form);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const items = field.children ?? [];
  const counted = field.ty === "counted array";
  const fixed = !counted && !!countField?.editable && field.ty.startsWith("array[");
  const used = fixed ? Math.min(Number(countField?.value ?? 0) || 0, items.length) : items.length;
  const run = async (change: TaskArrayEdit) => {
    setBusy(true);
    setError(null);
    try {
      await onRows(field, fixed ? countField!.path : null, change);
      return true;
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
      return false;
    } finally {
      setBusy(false);
    }
  };
  return { editable: counted || fixed, fixed, used, full: fixed && used >= items.length, busy, error, run };
}

function RowActions({ rows, index, noun }: { rows: ReturnType<typeof useRows>; index: number; noun: string }) {
  return <span className="task-form-row-actions">
    <button className="icon-btn small" disabled={rows.busy || rows.full} title={rows.full ? "Every slot is in use" : `Clone this ${noun} below it`} onClick={() => void rows.run({ kind: "clone", index })}><Copy size={13} /></button>
    <button className="icon-btn small danger" disabled={rows.busy} title={`Remove this ${noun}`} onClick={() => void rows.run({ kind: "remove", index })}><Trash2 size={13} /></button>
  </span>;
}

function RowsFoot({ rows, total, noun }: { rows: ReturnType<typeof useRows>; total: number; noun: string }) {
  if (!rows.editable) return null;
  return <div className="task-form-rows-foot">
    <button className="btn small" disabled={rows.busy || rows.full} title={rows.full ? "Every slot is in use" : undefined} onClick={() => void rows.run({ kind: "add" })}><Plus size={13} /> Add {noun}</button>
    {rows.fixed && <span className="muted small">{rows.used} of {total} slots used</span>}
    {rows.error && <span className="task-field-edit-error">{rows.error}</span>}
  </div>;
}

/**
 * An array: an editable table for structures, a list of values otherwise. Lists with a count (variable-length
 * lists, or fixed lists given `countField`) show their used rows with add, clone and remove; `used` only dims
 * unused slots of other fixed lists.
 */
function ArrayView({ field, countField, used }: { field: TaskFieldView; countField?: TaskFieldView; used?: number }) {
  const rows = useRows(field, countField);
  const items = field.children ?? [];
  const shownRows = rows.editable ? items.slice(0, rows.used) : items;
  const dimmed = (position: number) => !rows.editable && used !== undefined && position >= used;
  if (!shownRows.length) return <><div className="empty-note">None.</div><RowsFoot rows={rows} total={items.length} noun={items.length && !isComposite(items[0]) ? "entry" : "row"} /></>;
  if (isComposite(items[0])) {
    const columns = (items[0].children ?? []).filter((column) => !hidden(column));
    return <>
      <div className="task-form-table-wrap"><table className="task-form-table">
        <thead><tr><th>#</th>{columns.map((column) => <th key={column.name} title={column.name}>{fieldLabel(column.name)}</th>)}{rows.editable && <th />}</tr></thead>
        <tbody>{shownRows.map((item, row) => <tr key={item.name} className={dimmed(row) ? "unused" : undefined}><td className="muted">{row + 1}</td>{columns.map((column) => {
          const cell = child(item, column.name);
          return <td key={column.name}>{cell && (isComposite(cell) ? <ArrayView field={cell} /> : <Cell field={cell} />)}</td>;
        })}{rows.editable && <td className="task-form-row-actions-cell"><RowActions rows={rows} index={row} noun="row" /></td>}</tr>)}</tbody>
      </table></div>
      <RowsFoot rows={rows} total={items.length} noun="row" />
    </>;
  }
  // Long fixed lists without a count show their non-empty slots; short ones show every slot so entries can be filled in.
  const shown = rows.editable || items.length <= 8 ? shownRows : items.filter((item, position) => position < (used ?? 0) || (item.value !== "0" && item.value !== "false"));
  return <>
    <div className="task-form-list">
      {shown.map((item) => {
        const position = Number(item.name.slice(1, -1));
        return <span key={item.name} className={"task-form-list-item" + (dimmed(position) ? " unused" : "")}><span className="muted mono">{position + 1}</span><Cell field={item} />{rows.editable && <RowActions rows={rows} index={position} noun="entry" />}</span>;
      })}
      {shown.length < items.length && !rows.editable && <span className="muted small">{items.length - shown.length} empty slot{items.length - shown.length === 1 ? "" : "s"} (Advanced shows all)</span>}
    </div>
    <RowsFoot rows={rows} total={items.length} noun="entry" />
  </>;
}

/** A structure: its values in a grid, then nested arrays and structures. */
function StructView({ field }: { field: TaskFieldView }) {
  if (isArray(field)) return <ArrayView field={field} />;
  const children = (field.children ?? []).filter((entry) => !hidden(entry));
  return <>
    <Grid fields={children.filter((entry) => !isComposite(entry))} />
    {children.filter(isComposite).map((entry) => <div className="task-form-sub" key={entry.name}><div className="task-form-subtitle">{fieldLabel(entry.name)}</div>{isArray(entry) ? <ArrayView field={entry} /> : <StructView field={entry} />}</div>)}
  </>;
}

/** Friendship values listed by faction name. */
function FriendshipTable({ field }: { field: TaskFieldView }) {
  const { sets } = useContext(Form);
  const names = sets.task_friendship ?? new Map<number, string>();
  return <div className="task-form-friendships">{(field.children ?? []).map((entry, position) => <div key={entry.name} className={isDefault(entry) ? "unused" : undefined}><span title={`Friendship ${position}`}>{names.get(position) || `Friendship ${position}`}</span><Value field={entry} /></div>)}</div>;
}

/**
 * Class IDs (`CHARACTER_CLASS_CONFIG.character_class_id`) by race and class: pre-tier, then tiers 1–5
 * (null where a class has no such tier). Names come from the `task_occupation` set.
 */
const CLASS_GROUPS: { race: string; classes: { name: string; tiers: (number | null)[] }[] }[] = [
  { race: "Humans", classes: [{ name: "Vim", tiers: [null, 1, 2, 3, 13, 14] }, { name: "Lupin", tiers: [null, 4, 5, 6, 16, 17] }, { name: "Jadeon", tiers: [null, 7, 8, 9, 19, 20] }, { name: "Skysong", tiers: [null, 10, 11, 12, 22, 23] }, { name: "Modo", tiers: [null, 25, 26, 27, 28, 29] }, { name: "Incense", tiers: [null, 64, 65, 66, 67, 68] }] },
  { race: "Athans", classes: [{ name: "Balo", tiers: [null, 33, 34, 35, 36, 37] }, { name: "Arden", tiers: [null, 39, 40, 41, 42, 43] }, { name: "Rayan", tiers: [null, 45, 46, 47, 48, 49] }, { name: "Celan", tiers: [null, 51, 52, 53, 54, 55] }, { name: "Voida", tiers: [null, 56, 57, 58, 59, 60] }, { name: "Forta", tiers: [null, 96, 97, 98, 99, 100] }] },
  { race: "Etherkins", classes: [{ name: "Psychea", tiers: [null, 102, 103, 104, 105, 106] }, { name: "Kytos", tiers: [null, 108, 109, 110, 111, 112] }, { name: "Hydran", tiers: [null, 117, 118, 119, 120, 121] }, { name: "Seira", tiers: [null, 71, 72, 73, 74, 75] }, { name: "Gevrin", tiers: [null, 77, 78, 79, 80, 81] }, { name: "Sylia", tiers: [null, 83, 84, 85, 86, 87] }] },
  { race: "Deikins", classes: [{ name: "逐霜", tiers: [249, 250, 251, 252, 253, 254] }, { name: "惊岚", tiers: [null, 243, 244, 245, 246, 247] }, { name: "昭冥", tiers: [null, 235, 236, 237, 238, 239] }, { name: "涅羽", tiers: [null, 228, 229, 230, 231, 232] }, { name: "百灵", tiers: [null, 221, 222, 223, 224, 225] }, { name: "释罗", tiers: [null, 215, 216, 217, 218, 219] }] },
];
const TIER_HEADINGS = ["Pre-tier", "Tier 1", "Tier 2", "Tier 3", "Tier 4", "Tier 5"];
const GROUPED_CLASSES = new Set(CLASS_GROUPS.flatMap((group) => group.classes.flatMap((entry) => entry.tiers)).filter((id): id is number => id !== null));

/** A checkbox for several classes at once: ticked when all are selected, dashed when some are. */
function GroupCheck({ ids, selected, disabled, onChange, title }: { ids: number[]; selected: number[]; disabled: boolean; onChange: (ids: number[], on: boolean) => void; title: string }) {
  const on = ids.filter((id) => selected.includes(id)).length;
  return <input type="checkbox" checked={on === ids.length && on > 0} ref={(input) => { if (input) input.indeterminate = on > 0 && on < ids.length; }} disabled={disabled} title={title} onChange={() => onChange(ids, on < ids.length)} />;
}

/** Class requirements: races and classes with a checkbox per tier, then any other classes elements.data defines. */
function ClassChecklist({ list, count }: { list: TaskFieldView; count: TaskFieldView }) {
  const { onBatch, onEditSet, sets, classes } = useContext(Form);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const slots = list.children ?? [];
  const used = Number(count.value ?? 0);
  const selected = slots.slice(0, used).map((slot) => Number(slot.value));
  const names = sets.task_occupation ?? new Map<number, string>();
  const known = new Map<number, string>(classes.map(([id, name]) => [id, name]));
  const label = (id: number) => names.get(id) ?? known.get(id) ?? `Class ${id}`;
  const hint = (id: number) => `Class ID ${id}${known.get(id) && known.get(id) !== label(id) ? ` · ${known.get(id)}` : ""}`;
  const needle = filter.trim().toLowerCase();
  const editable = !!slots[0]?.editable && count.editable;
  const disabled = busy || !editable;
  /** Ticks or clears classes; kept classes keep their order and new ones follow by ID. */
  const change = async (ids: number[], on: boolean) => {
    const next = on ? [...selected, ...ids.filter((id) => !selected.includes(id)).sort((left, right) => left - right)] : selected.filter((id) => !ids.includes(id));
    if (next.length > slots.length) {
      setError(`At most ${slots.length} classes fit in a task; this would select ${next.length}.`);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const values = slots.map((slot, position) => ({ fieldPath: slot.path, value: String(next[position] ?? 0) }));
      const plural = ids.length === 1 ? "requirement" : "requirements";
      await onBatch([...values, { fieldPath: count.path, value: String(next.length) }], on ? `Add class ${plural}` : `Remove class ${plural}`);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  };
  const matches = (race: string, name: string) => !needle || `${race} ${name}`.toLowerCase().includes(needle);
  const others = [...new Set([...known.keys(), ...names.keys(), ...selected])].filter((id) => !GROUPED_CLASSES.has(id)).sort((left, right) => left - right)
    .filter((id) => !needle || `${id} ${label(id)} ${known.get(id) ?? ""}`.toLowerCase().includes(needle));
  return <div className="task-form-classes">
    <div className="task-form-classes-head">
      <span className="muted small">{selected.length} of at most {slots.length} selected{selected.length === 0 ? " · none means any class can take the quest" : ""}</span>
      {onEditSet && <button className="link" onClick={() => onEditSet("task_occupation")}>Edit names…</button>}
      <input className="sf-control" placeholder="Filter races and classes…" value={filter} onChange={(event) => setFilter(event.target.value)} />
    </div>
    <div className="task-form-table-wrap">
      <table className="task-form-table task-form-class-table">
        <thead><tr><th>Class</th>{TIER_HEADINGS.map((heading) => <th key={heading}>{heading}</th>)}</tr></thead>
        {CLASS_GROUPS.map((group) => {
          const shown = group.classes.filter((entry) => matches(group.race, entry.name));
          if (!shown.length) return null;
          const raceIds = group.classes.flatMap((entry) => entry.tiers).filter((id): id is number => id !== null);
          return <tbody key={group.race}>
            <tr className="task-form-class-race"><th colSpan={TIER_HEADINGS.length + 1}><label><GroupCheck ids={raceIds} selected={selected} disabled={disabled} onChange={(ids, on) => void change(ids, on)} title={`Every ${group.race} class and tier`} />{group.race}</label></th></tr>
            {shown.map((entry) => {
              const ids = entry.tiers.filter((id): id is number => id !== null);
              return <tr key={entry.name}>
                <td><label className="task-form-class-name"><GroupCheck ids={ids} selected={selected} disabled={disabled} onChange={(next, on) => void change(next, on)} title={`Every ${entry.name} tier`} />{entry.name}</label></td>
                {entry.tiers.map((id, tier) => <td key={tier}>{id !== null && <label className={"task-form-class-tier" + (selected.includes(id) ? " checked" : "")} title={`${label(id)} · ${hint(id)}`}>
                  <input type="checkbox" checked={selected.includes(id)} disabled={disabled} onChange={(event) => void change([id], event.target.checked)} /><span className="mono muted">{id}</span>
                </label>}</td>)}
              </tr>;
            })}
          </tbody>;
        })}
      </table>
    </div>
    {others.length > 0 && <>
      <div className="task-form-subtitle task-form-classes-other">Other classes</div>
      <div className="task-form-class-list">{others.map((id) => <label key={id} className={selected.includes(id) ? "checked" : undefined} title={hint(id)}>
        <input type="checkbox" checked={selected.includes(id)} disabled={disabled} onChange={(event) => void change([id], event.target.checked)} />
        <span className="mono muted">{id}</span><span className="truncate">{label(id)}</span>{known.get(id) && known.get(id) !== label(id) && <span className="muted small truncate">{known.get(id)}</span>}
      </label>)}</div>
    </>}
    {error && <div className="task-field-edit-error">{error}</div>}
  </div>;
}

/** NPC talks: each window's text with its player options. */
const isWindow = (value?: string) => !!value && value !== "0" && value !== "4294967295";
/** NPC functions an option can run (SERVICE_TYPE in ExpTypes.h, after the 0x80000000 flag). */
const SERVICES = ["Talk", "Sell", "Buy", "Repair", "Install", "Uninstall", "Give quest", "Complete quest", "Give quest item", "Skills", "Heal", "Teleport", "Transport", "Proxy", "Storage", "Make", "Decompose", "Back", "Exit", "Storage password", "Identify", "Give up quest", "War tower", "Reset properties", "Bind equipment", "Destroy equipment", "Undo destroy equipment", "War archers", "Item trade", "Soul meld", "Consign", "Transcription"];
/** Functions whose parameter is a quest ID (cloning a quest moves these to the copy). */
const QUEST_SERVICES = new Set([0, 6, 7, 8, 21]);
function optionTarget(value?: string) {
  const id = Number(value ?? 0);
  if (id >= 0x80000000) return `→ ${SERVICES[id - 0x80000000] ?? "function"} (${id - 0x80000000})`;
  return id ? `→ window ${id}` : "→ close";
}
function OptionView({ option, taskId }: { option: TaskFieldView; taskId: number }) {
  const text = child(option, "text");
  const parameter = child(option, "parameter");
  const id = Number(child(option, "id")?.value ?? 0);
  const service = id >= 0x80000000 ? id - 0x80000000 : null;
  const quest = service !== null && QUEST_SERVICES.has(service);
  // Window links and most functions leave the parameter at zero; show it whenever it means something.
  const showParameter = parameter && (quest || parameter.value !== "0");
  return <div className="task-form-option">
    <span className="muted">▸</span>
    {text && <Value field={text} />}
    <span className="task-form-option-target muted small mono" title={`Option ID ${id}`}>{optionTarget(String(id))}</span>
    {showParameter && <span className="task-form-option-parameter" title={dotted(parameter.path)}>
      <span className="muted small">{quest ? "Quest" : "Parameter"}</span>
      <Cell field={parameter} />
      {quest && Number(parameter.value) === taskId && <span className="muted small">this quest</span>}
    </span>}
  </div>;
}
function DialogsView({ field, taskId }: { field: TaskFieldView; taskId: number }) {
  return <>{(field.children ?? []).map((talk) => {
    const windows = child(talk, "windows")?.children ?? [];
    const prompt = child(talk, "prompt");
    return <div className="task-form-sub" key={talk.name}>
      <div className="task-form-subtitle">{fieldLabel(talk.name)} <span className="muted small">· {windows.length} window{windows.length === 1 ? "" : "s"}</span></div>
      {prompt && <Grid fields={[prompt]} />}
      {windows.map((window) => {
        const text = child(window, "text");
        const options = child(window, "options")?.children ?? [];
        return <div className="task-form-window" key={window.name}>
          <div className="muted small">Window {child(window, "id")?.value}{isWindow(child(window, "parent_id")?.value) ? ` · opened from window ${child(window, "parent_id")?.value}` : ""}</div>
          {text && <Value field={text} wide />}
          {options.map((option) => <OptionView key={option.name} option={option} taskId={taskId} />)}
        </div>;
      })}
    </div>;
  })}</>;
}

// ---------------------------------------------------------------- rewards

/** AWARD_DATA split into sub-tabs like the official editors; "Other" takes the rest. */
const AWARD_TABS: { key: string; label: string; fields: string[] }[] = [
  { key: "dividends", label: "Dividends", fields: ["gold", "experience", "spirit", "reputation", "contribution", "family_contribution", "prosperity", "fury_limit", "experience_coefficient", "experience_coefficient_2", "experience_coefficient_3", "double_experience_time", "circle_group_points", "battle_score", "battle_score_award", "bonus", "king_score", "fengshen_experience", "open_star_soul", "star_soul_value"] },
  { key: "candidates", label: "Candidate items", fields: [] },
  { key: "storage", label: "Storage", fields: ["storehouse_size", "faction_storehouse_size", "inventory_size", "pocket_size", "pet_inventory_size", "mount_inventory_size"] },
  { key: "faction", label: "Faction", fields: ["faction_gold_note", "faction_grass", "faction_mine", "faction_monster_core", "faction_monster_food", "faction_money", "building_progress", "extra_faction_gold_note", "extra_faction_grass", "extra_faction_mine", "extra_faction_monster_core", "extra_faction_monster_food", "extra_faction_money"] },
  { key: "travel", label: "Travel", fields: ["teleport_world_id", "teleport_position", "travel_item_id", "travel_time", "travel_speed", "travel_path", "camera_move_id", "animation_id"] },
  { key: "spawn", label: "Spawn", fields: ["monster_controller", "trigger_controller", "monster_control_count", "random_monster_control", "monster_controls", "extra_monster_control_count", "random_extra_monster_control", "phase_count", "phases"] },
  { key: "quests", label: "Quests", fields: ["new_task_id", "terminate_task_count", "terminate_task_ids", "clear_count_task", "clear_no_key_active_task", "check_global_finish_count", "global_finish_count_precondition", "check_global_expression", "multi_global_key", "new_period", "new_relay_station"] },
  { key: "character", label: "Character", fields: ["title_id", "pk_value", "reset_pk_value", "divorce", "new_profession", "rebirth_count", "rebirth_faction", "set_produce_skill", "produce_skill_experience", "buff_id", "buff_level", "set_cultivation", "cultivation", "clear_cultivation_skill", "clear_skill_points", "clear_book_points", "transform_id", "transform_duration", "transform_level", "transform_experience_level", "transform_cover", "fengshen_trial", "open_soul_equipment", "clear_experience_cooldown", "reset_battle_score", "award_selected_role", "selected_role", "master_moral", "leave_master", "deviate_master", "apprentice_gets_master_experience", "master_gets_moral", "family_skill_proficiency", "family_skill_level", "family_skill_index", "family_monster_record_index", "family_value_index", "family_value"] },
  { key: "messages", label: "Messages", fields: ["send_message", "message_channel", "send_extra_message", "extra_message_channel", "extra_tribute"] },
  { key: "friendship", label: "Friendship", fields: ["friendship_reset_selection"] },
];
const PLACED_AWARD = new Set(AWARD_TABS.flatMap((tab) => tab.fields).concat(["candidate_count", "candidates", "faction_candidate_count", "faction_candidates", "extra_candidate_count", "extra_candidates", "friendships", "friendships_v165"]));

function CandidateItems({ award }: { award: TaskFieldView }) {
  const lists = [["candidates", "Candidates"], ["faction_candidates", "Faction candidates"], ["extra_candidates", "Extra candidates"]] as const;
  const available = lists.filter(([name]) => child(award, name));
  const [which, setWhich] = useState<string>("candidates");
  const [chosen, setChosen] = useState(0);
  const list = child(award, which);
  const candidates = list?.children ?? [];
  const current = Math.max(0, Math.min(chosen, candidates.length - 1));
  const candidate = candidates[current];
  const items = child(candidate, "items");
  const empty = { name: which, path: [], ty: "", children: [] } as unknown as TaskFieldView;
  const rows = useRows(list ?? empty);
  const run = async (change: TaskArrayEdit, select: number) => {
    if (await rows.run(change)) setChosen(Math.max(0, select));
  };
  return <div className="task-form-candidates">
    {available.length > 1 && <div className="segmented">{available.map(([name, label]) => <button key={name} className={which === name ? "active" : ""} onClick={() => { setWhich(name); setChosen(0); }}>{label} ({child(award, name)?.children?.length ?? 0})</button>)}</div>}
    {list && rows.editable && <div className="task-form-rows-foot top">
      <button className="btn small" disabled={rows.busy} onClick={() => void run({ kind: "add" }, candidates.length)}><Plus size={13} /> Add candidate</button>
      {candidate && <button className="btn small" disabled={rows.busy} onClick={() => void run({ kind: "clone", index: current }, current + 1)}><Copy size={13} /> Clone candidate {current + 1}</button>}
      {candidate && <button className="btn small danger-outline" disabled={rows.busy} onClick={() => void run({ kind: "remove", index: current }, Math.min(current, candidates.length - 2))}><Trash2 size={13} /> Remove candidate {current + 1}</button>}
      {rows.error && <span className="task-field-edit-error">{rows.error}</span>}
    </div>}
    {!candidates.length ? <div className="empty-note">No candidate items.</div> : <div className="task-form-candidate-body">
      <div className="task-form-candidate-list">{candidates.map((entry, position) => <button key={entry.name} className={position === current ? "active" : ""} onClick={() => setChosen(position)}>
        <span>Candidate {position + 1}</span><span className="muted small">{child(entry, "item_count")?.value ?? 0} items{child(entry, "random_choice")?.value === "true" ? " · random" : ""}</span>
      </button>)}</div>
      <div className="task-form-candidate-detail">
        {candidate && <Grid fields={[child(candidate, "random_choice"), child(candidate, "item_count")].filter((entry): entry is TaskFieldView => !!entry)} />}
        {items && <ArrayView field={items} />}
      </div>
    </div>}
  </div>;
}

function AwardEditor({ award }: { award: TaskFieldView }) {
  const [tab, setTab] = useState("dividends");
  const fields = (award.children ?? []).filter((entry) => !hidden(entry));
  const named = (name: string) => fields.find((entry) => entry.name === name);
  const friendships = named("friendships") ?? named("friendships_v165");
  const other = fields.filter((entry) => !PLACED_AWARD.has(entry.name));
  const tabs = [...AWARD_TABS, { key: "other", label: "Other", fields: other.map((entry) => entry.name) }];
  const used = (key: string) => {
    if (key === "candidates") return ["candidates", "faction_candidates", "extra_candidates"].some((name) => { const entry = named(name); return !!entry && !isDefault(entry); });
    if (key === "friendship" && friendships && !isDefault(friendships)) return true;
    return tabs.find((entry) => entry.key === key)!.fields.some((name) => { const entry = named(name); return !!entry && !isDefault(entry); });
  };
  const current = tabs.find((entry) => entry.key === tab) ?? tabs[0];
  const shown = current.fields.map(named).filter((entry): entry is TaskFieldView => !!entry);
  return <div className="task-award">
    <div className="task-award-tabs" role="tablist">{tabs.map((entry) => <button key={entry.key} role="tab" aria-selected={entry.key === current.key} className={entry.key === current.key ? "active" : ""} onClick={() => setTab(entry.key)}>{entry.label}{used(entry.key) && <span className="task-award-dot" title="Holds values" />}</button>)}</div>
    <div className="task-award-body">
      {current.key === "candidates" ? <CandidateItems award={award} /> : <>
        <Grid fields={shown.filter((entry) => !isComposite(entry))} />
        {shown.filter(isComposite).map((entry) => <div className="task-form-sub" key={entry.name}><div className="task-form-subtitle">{fieldLabel(entry.name)}</div>{isArray(entry) ? <ArrayView field={entry} /> : <StructView field={entry} />}</div>)}
        {current.key === "friendship" && friendships && <FriendshipTable field={friendships} />}
      </>}
    </div>
  </div>;
}

const AWARD_SOURCES: { key: string; label: string; field: string; scale?: "ratios" | "counts" }[] = [
  { key: "success", label: "Award on success", field: "success_award" },
  { key: "failure", label: "Award on failure", field: "failure_award" },
  { key: "ratio_success", label: "Ratio awards (success)", field: "success_ratio_awards", scale: "ratios" },
  { key: "ratio_failure", label: "Ratio awards (failure)", field: "failure_ratio_awards", scale: "ratios" },
  { key: "item_success", label: "Item-count awards (success)", field: "success_item_awards", scale: "counts" },
  { key: "item_failure", label: "Item-count awards (failure)", field: "failure_item_awards", scale: "counts" },
  { key: "count_success", label: "Finish-count awards (success)", field: "success_count_awards", scale: "counts" },
  { key: "count_failure", label: "Finish-count awards (failure)", field: "failure_count_awards", scale: "counts" },
];

function RewardsView({ fields }: { fields: Map<string, TaskFieldView> }) {
  const sources = AWARD_SOURCES.filter((source) => fields.has(source.field));
  const [selected, setSelected] = useState(sources[0]?.key ?? "success");
  const [entry, setEntry] = useState(0);
  const source = sources.find((candidate) => candidate.key === selected) ?? sources[0];
  const container = source ? fields.get(source.field) : undefined;
  const awards = source?.scale ? child(container, "awards")?.children ?? [] : [];
  const award = source?.scale ? awards[Math.min(entry, awards.length - 1)] : container;
  const scale = source?.scale ? child(container, source.scale) : undefined;
  const types = ["fixed.award_type_s", "fixed.award_type_f", "fixed.special_award"].map((path) => fields.get(path)).filter((field): field is TaskFieldView => !!field);
  return <>
    <fieldset className="task-form-group wide">
      <legend>Reward selector</legend>
      <div className="task-rewards-head">
        <div className="task-rewards-sources">{sources.map((candidate) => {
          const value = fields.get(candidate.field);
          return <label key={candidate.key} className={candidate.key === source?.key ? "active" : undefined}>
            <input type="radio" name="task-award-source" checked={candidate.key === source?.key} onChange={() => { setSelected(candidate.key); setEntry(0); }} />
            {candidate.label}{value && !isDefault(value) && <span className="task-award-dot" title="Holds values" />}
          </label>;
        })}</div>
        <div className="task-rewards-types"><Grid fields={types} /></div>
      </div>
    </fieldset>
    {source?.scale && container && <fieldset className="task-form-group wide">
      <legend>{source.scale === "ratios" ? "Ratios" : "Counts"}</legend>
      <Grid fields={[child(container, "scale_count"), child(container, "item_id")].filter((field): field is TaskFieldView => !!field)} />
      {scale && <ArrayView field={scale} used={Number(child(container, "scale_count")?.value ?? 0)} />}
      {awards.length > 0 && <div className="segmented task-rewards-entries">{awards.map((item, position) => <button key={item.name} className={position === entry ? "active" : ""} onClick={() => setEntry(position)}>Entry {position + 1}{scale?.children?.[position] ? ` · ${scale.children[position].value}` : ""}</button>)}</div>}
    </fieldset>}
    {award ? <fieldset className="task-form-group wide"><legend>{source?.label}{source?.scale ? ` · entry ${entry + 1}` : ""}</legend><AwardEditor key={`${source?.key}:${entry}`} award={award} /></fieldset>
      : <div className="empty-note">{source?.scale ? "No entries. The number of entries (scale count) is set in Advanced." : "No award."}</div>}
  </>;
}

// ---------------------------------------------------------------- layout

type Group =
  | { title: string; grid: string[] }
  | { title: string; table: string; count?: string; before?: string[] }
  | { title: string; list: string; count?: string; before?: string[] }
  | { title: string; classes: string; count: string; before?: string[] }
  | { title: string; friendship: string; before?: string[] }
  | { title: string; dialogs: string }
  | { title: string; rest: (name: string) => boolean };

const f = (...names: string[]) => names.map((name) => `fixed.${name}`);

const LAYOUT: Record<Exclude<TaskFormTab, "advanced" | "rewards">, Group[]> = {
  general: [
    { title: "Identity", grid: [...f("id", "name", "task_type", "method", "finish_type", "rank", "suitable_level", "display_type", "recommend_type", "dynamic_task_type", "special_award", "storage_weight", "has_signature"), "signature"] },
    { title: "Time", grid: f("time_limit", "absolute_time", "absolute_fail", "absolute_fail_time", "avail_frequency", "time_interval", "max_finish_count", "finish_clear_time", "dynamic_finish_clear_time", "finish_time_type", "fail_after_logout", "logout_fail_time") },
    { title: "Flags", grid: f("choose_one", "rand_one", "execute_child_in_order", "parent_also_fail", "parent_also_succ", "can_give_up", "can_redo", "can_redo_after_failure", "clear_as_give_up", "need_record", "fail_as_player_die", "auto_deliver", "deliver_window_mode", "death_trig", "manual_trig", "must_shown", "clear_acquired", "show_prompt", "key_task", "skill_task", "can_seek_out", "show_direction", "hidden", "marriage", "faction", "kermis", "shared_by_family", "rec_finish_count", "rec_finish_count_global", "life_again_reset", "prentice_task", "clear_some_illegal_states", "clear_xp_cd", "script_open_task") },
    { title: "Timetable", table: "timetables" },
    { title: "Hierarchy", grid: f("hierarchy_parent", "hierarchy_previous_sibling", "hierarchy_next_sibling", "hierarchy_first_child") },
  ],
  locations: [
    { title: "Delivery", grid: f("deliver_npc", "award_npc", "action_npc", "action_id", "auto_deliver", "deliver_in_zone", "deliver_world", "deliver_min_vertex", "deliver_max_vertex") },
    { title: "Transfer on accept", grid: f("trans_to", "trans_world_id", "trans_point") },
    { title: "Fail when leaving a zone", grid: f("out_zone_fail", "out_zone_world_id", "out_zone_min_vertex", "out_zone_max_vertex") },
    { title: "Fail when entering a zone", grid: f("enter_zone_fail", "enter_zone_world_id", "enter_zone_min_vertex", "enter_zone_max_vertex") },
    { title: "Receivers", grid: f("max_receiver", "clear_receiver_type", "clear_receiver_time_interval") },
    { title: "Camera and animation", grid: f("camera_move", "animation", "tiny_game_id") },
  ],
  requirements: [
    { title: "Character", grid: f("premise_level_min", "premise_level_max", "show_by_level", "gender", "show_by_gender", "premise_period", "show_by_period", "premise_faction", "show_by_faction", "premise_faction_master", "premise_spouse", "show_by_spouse", "talisman_value_min", "talisman_value_max", "pk_value_min", "pk_value_max", "premise_gm") },
    { title: "Classes", classes: "fixed.occupations", count: "fixed.occupation_count", before: f("show_by_occupation") },
    { title: "Quests finished first", list: "fixed.premise_tasks", count: "fixed.premise_task_count", before: f("premise_task_count", "show_by_premise_task") },
    { title: "Quests finished a number of times", table: "fixed.premise_finish_tasks", count: "fixed.premise_finish_task_count", before: f("premise_finish_task_count") },
    { title: "Global and co-task", grid: f("premise_global_count", "premise_global_task", "premise_cotask", "cotask_condition") },
    { title: "Mutually exclusive quests", list: "fixed.mutex_tasks", count: "fixed.mutex_task_count", before: f("mutex_task_count") },
    { title: "Items required", table: "premise_items", before: f("show_by_items", "premise_items_not_taken") },
    { title: "Items given on accept", table: "given_items", before: f("given_common_item_count", "given_task_item_count") },
    { title: "Titles required", table: "premise_titles" },
    { title: "Monsters summoned on accept", table: "summoned_monsters", before: f("summon_mode", "random_summoned_monster") },
    { title: "Costs and scores", grid: f("premise_deposit", "show_by_deposit", "premise_reputation", "reputation_deposit", "show_by_reputation", "premise_contribution", "deposit_contribution", "premise_family_contrib", "premise_family_contrib_max", "deposit_family_contrib", "premise_battle_score_min", "premise_battle_score_max", "deposit_battle_score", "premise_sj_battle_score", "sj_deposit_battle_score") },
    { title: "Friendship", friendship: "fixed.premise_friendship", before: f("friendship_deposit") },
    { title: "Team", grid: f("teamwork", "receive_by_team", "shared_task", "shared_achieved", "check_teammate", "teammate_distance", "all_fail", "captain_change_all_fail", "captain_fail", "captain_success", "success_distance", "all_success", "dismiss_as_self_fail", "receive_check_members", "receive_member_distance", "count_by_member_position", "count_member_distance", "show_by_team", "share_work") },
    { title: "Team members", table: "team_members" },
    { title: "Master and apprentice", table: "fixed.master_apprentice_tasks", count: "fixed.master_apprentice_task_count", before: f("master", "prentice", "master_moral", "master_apprentice_task", "out_master_task", "master_apprentice_task_count") },
    { title: "Family", grid: f("in_family", "family_header", "family_skill_level_min", "family_skill_level_max", "family_skill_proficiency_min", "family_skill_proficiency_max", "family_skill_index", "family_monster_record_index", "family_monster_record_min", "family_monster_record_max", "family_value_index", "deposit_family_value", "family_value_min", "family_value_max") },
    { title: "Rebirth", grid: f("check_life_again", "spouse_again", "life_again_count", "life_again_count_compare") },
    { title: "More requirements", rest: (name) => /^(premise_|show_by_|living_skill|pet_|build_|interaction_object_id|create_role|exp_must|consume_|script_open)/.test(name) },
  ],
  objectives: [
    { title: "Completion", grid: f("method", "finish_type", "finish_level", "finish_achievement", "friend_num", "wait_time", "show_wait_time", "fixed_type", "fixed_time", "disable_finish_dialog", "script_finish_task", "total_case_add_min", "total_case_add_max") },
    { title: "Monsters to kill", table: "monsters_wanted", before: f("summon_monster_mode") },
    { title: "Items to collect", table: "items_wanted" },
    { title: "Collect", grid: f("gold_wanted", "auto_move_for_collect_num_items", "faction_gold_note_wanted", "faction_grass_wanted", "faction_mine_wanted", "faction_monster_core_wanted", "faction_monster_food_wanted", "faction_money_wanted", "building_id_wanted", "building_level_wanted") },
    { title: "Objects to interact with", table: "interaction_objects_wanted" },
    { title: "Sites", grid: f("reach_site_id", "reach_site_min", "reach_site_max", "leave_site_id", "leave_site_min", "leave_site_max", "auto_move_for_reach_fixed_site", "auto_move_dest_pos", "auto_move_dest_pos_name", "interaction_reach_site_id", "interaction_reach_item_id", "interaction_reach_site_min", "interaction_reach_site_max", "interaction_leave_site_id", "interaction_leave_item_id", "interaction_leave_site_min", "interaction_leave_site_max") },
    { title: "Escort", grid: f("npc_to_protect", "protect_time_len", "npc_moving", "npc_dest_site") },
    { title: "Titles to earn", list: "fixed.title_wanted", count: "fixed.title_wanted_num", before: f("title_wanted_num") },
    { title: "Failure", grid: f("kill_monster_fail", "have_item_fail", "have_item_fail_not_take_off", "not_have_item_fail") },
    { title: "Monsters that fail the quest", list: "fixed.kill_fail_monsters", count: "fixed.kill_fail_monster_count", before: f("kill_fail_monster_count") },
    { title: "Items that fail the quest when held", list: "fixed.have_fail_items", count: "fixed.have_fail_item_count", before: f("have_fail_item_count") },
    { title: "Items that fail the quest when missing", list: "fixed.not_have_fail_items", count: "fixed.not_have_fail_item_count", before: f("not_have_fail_item_count") },
  ],
  texts: [
    { title: "Texts", grid: ["texts.description", "texts.success_text", "texts.failure_text", "texts.tribute", "texts.hint", "texts.can_deliver_text"] },
    { title: "Dialogs", dialogs: "dialogs" },
  ],
};

/** Every header path a tab lays out, so "More requirements" lists only the rest. */
const PLACED = new Set(Object.values(LAYOUT).flat().flatMap((group) => [
  ...("grid" in group ? group.grid : []),
  ...("before" in group && group.before ? group.before : []),
  ...("list" in group ? [group.list, group.count ?? ""] : []),
  ...("classes" in group ? [group.classes, group.count] : []),
  ...("table" in group ? [group.table, group.count ?? ""] : []),
  ...("friendship" in group ? [group.friendship] : []),
]).concat(f("award_type_s", "award_type_f", "special_award", "life_again_one_occupation", "life_again_two_occupation", "life_again_thr_occupation")));

/** The named set of a field, by its last name: enums, masks (`mask`), and class lists. */
export function taskValueSet(name: string): { key: string; mask: boolean } | null {
  if (ENUM_FIELDS[name]) return { key: ENUM_FIELDS[name], mask: false };
  if (MASK_FIELDS[name]) return { key: MASK_FIELDS[name], mask: true };
  if (name === "occupations" || name === "occupation") return { key: "task_occupation", mask: false };
  return null;
}

const TAB_LABEL = Object.fromEntries(TASK_FORM_TABS.map((entry) => [entry.key, entry.label])) as Record<TaskFormTab, string>;
const labelParts = (parts: string[]) => parts.filter((part) => part !== "awards").map(fieldLabel).join(" › ");

/**
 * Where a task field sits in the form: its tab, group and label. Used by the advanced search to name
 * fields (`Objectives › Monsters to kill › Monster ID`) and to open a result on the right tab.
 * Paths have no indexes; `any:award:…` and `any:dialog:…` stand for every award or dialog.
 */
export function describeTaskField(path: string): { tab: TaskFormTab; tabLabel: string; group: string; label: string } {
  const result = (tab: TaskFormTab, group: string, label: string) => ({ tab, tabLabel: TAB_LABEL[tab], group, label });
  const any = path.match(/^any:(award|dialog):(.+)$/);
  if (any) return any[1] === "award" ? result("rewards", "Any award", labelParts(any[2].split("."))) : result("texts", "Any dialog", labelParts(any[2].split(".")));
  const parts = path.split(".");
  if (parts[0] === "texts") return result("texts", "Texts", labelParts(parts.slice(1)));
  if (parts[0] === "dialogs") return result("texts", `Dialogs › ${fieldLabel(parts[1] ?? "")}`, labelParts(parts.slice(2)));
  const source = AWARD_SOURCES.find((entry) => entry.field === parts[0]);
  if (source) return result("rewards", source.label, labelParts(parts.slice(1)));
  for (const [tab, groups] of Object.entries(LAYOUT) as [Exclude<TaskFormTab, "advanced" | "rewards">, Group[]][]) {
    for (const group of groups) {
      const exact = [...("grid" in group ? group.grid : []), ...("before" in group && group.before ? group.before : [])];
      if (exact.includes(path)) return result(tab, group.title, fieldLabel(parts[parts.length - 1]));
      const list = "table" in group ? group.table : "list" in group ? group.list : "classes" in group ? group.classes : "friendship" in group ? group.friendship : null;
      if (list && (path === list || path.startsWith(list + "."))) return result(tab, group.title, path === list ? fieldLabel(parts[parts.length - 1]) : labelParts(path.slice(list.length + 1).split(".")));
      if ("count" in group && group.count === path) return result(tab, group.title, fieldLabel(parts[parts.length - 1]));
    }
  }
  if (["fixed.award_type_s", "fixed.award_type_f", "fixed.special_award"].includes(path)) return result("rewards", "Reward selector", fieldLabel(parts[1]));
  if (parts[0] === "fixed") {
    const rest = LAYOUT.requirements.find((group) => "rest" in group);
    if (rest && "rest" in rest && rest.rest(parts[1])) return result("requirements", rest.title, labelParts(parts.slice(1)));
    return result("advanced", "Header", labelParts(parts.slice(1)));
  }
  return result("advanced", "Other", labelParts(parts));
}

/** The quest as a form: labelled groups per tab, like the official editors. */
export function TaskForm({ detail, tab, onEdit, onBatch, onRows, onEditSet, setsVersion, renderReference, referencedBy }: Props) {
  const fields = useMemo(() => index(detail.fields), [detail]);
  const [sets, setSets] = useState<Record<string, Map<number, string>>>({});
  const [classes, setClasses] = useState<[number, string][]>([]);
  const [loadedVersion, setLoadedVersion] = useState(setsVersion);
  useEffect(() => {
    let cancelled = false;
    if (setsVersion !== loadedVersion) {
      setCache.clear();
      setLoadedVersion(setsVersion);
    }
    void Promise.all(SET_KEYS.map(async (key) => [key, await loadSet(key)] as const)).then((loaded) => { if (!cancelled) setSets(Object.fromEntries(loaded)); });
    characterClasses().then((list) => { if (!cancelled) setClasses(list); }).catch(() => {});
    return () => { cancelled = true; };
  }, [setsVersion]); // eslint-disable-line react-hooks/exhaustive-deps
  const context = useMemo(() => ({ onEdit, onBatch, onRows, onEditSet, renderReference, sets, classes }), [classes, onBatch, onEdit, onEditSet, onRows, renderReference, sets]);
  const get = (path: string) => fields.get(path);
  const all = (paths: string[] = []) => paths.map(get).filter((field): field is TaskFieldView => !!field);

  let content: ReactNode;
  if (tab === "rewards") {
    content = <RewardsView fields={fields} />;
  } else {
    content = LAYOUT[tab].map((group) => {
      let body: ReactNode = null;
      const before = "before" in group ? <Grid fields={all(group.before)} /> : null;
      if ("grid" in group) {
        const shown = all(group.grid);
        if (shown.length) body = <Grid fields={shown} />;
      } else if ("table" in group) {
        const field = get(group.table);
        if (field) body = <>{before}<ArrayView field={field} countField={group.count ? get(group.count) : undefined} /></>;
      } else if ("list" in group) {
        const field = get(group.list);
        if (field) body = <>{before}<ArrayView field={field} countField={group.count ? get(group.count) : undefined} /></>;
      } else if ("classes" in group) {
        const list = get(group.classes);
        const count = get(group.count);
        if (list && count) body = <>{before}<ClassChecklist list={list} count={count} /></>;
      } else if ("friendship" in group) {
        const field = get(group.friendship);
        if (field) body = <>{before}<FriendshipTable field={field} /></>;
      } else if ("dialogs" in group) {
        const field = get(group.dialogs);
        if (field) body = <DialogsView field={field} taskId={detail.id} />;
      } else {
        const rest = (get("fixed")?.children ?? []).filter((field) => !PLACED.has(dotted(field.path)) && !hidden(field) && group.rest(field.name));
        if (rest.length) body = <><Grid fields={rest.filter((field) => !isComposite(field))} />{rest.filter(isComposite).map((field) => <div className="task-form-sub" key={field.name}><div className="task-form-subtitle">{fieldLabel(field.name)}</div><ArrayView field={field} /></div>)}</>;
      }
      return body && <fieldset className={"task-form-group" + ("grid" in group && group.grid.length > 8 || "dialogs" in group || "classes" in group || "friendship" in group || "table" in group || "rest" in group ? " wide" : "")} key={group.title}><legend>{group.title}</legend>{body}</fieldset>;
    });
  }
  return <Form.Provider value={context}>
    <div className="task-form">
      {content}
      {tab === "general" && referencedBy}
    </div>
  </Form.Provider>;
}
