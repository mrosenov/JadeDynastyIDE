import { useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronDown, CircleAlert, Copy, Download, Hash, Link2, ListFilter, Loader2, Plus, Search, Sparkles, Trash2, X } from "lucide-react";
import { cancelTaskSearch, searchTasksAdvanced, taskSearchFields, taskSearchProgress } from "../elements/api";
import type { TaskExportTarget, TaskSearchCondition, TaskSearchField, TaskSearchHit, TaskSearchOp, TaskSearchQuery, TaskSearchResults, TaskSearchScope } from "../elements/types";
import { describeTaskField, loadSet, taskValueSet, type TaskFormTab } from "./TaskForm";

const count = (value: number) => value.toLocaleString();
const plainPath = (field: string) => field.replace(/\[\d+\]/g, "");
const hitKey = (hit: TaskSearchHit) => `${hit.pack}:${hit.root}:${hit.path.join(".")}`;

type Kind = TaskSearchField["kind"];
const OPS: { op: TaskSearchOp; label: string; kinds: Kind[]; value: boolean }[] = [
  { op: "eq", label: "=", kinds: ["int", "float", "bool", "text"], value: true },
  { op: "ne", label: "≠", kinds: ["int", "float", "bool", "text"], value: true },
  { op: "lt", label: "<", kinds: ["int", "float"], value: true },
  { op: "le", label: "≤", kinds: ["int", "float"], value: true },
  { op: "gt", label: ">", kinds: ["int", "float"], value: true },
  { op: "ge", label: "≥", kinds: ["int", "float"], value: true },
  { op: "in", label: "is one of", kinds: ["int", "float", "text"], value: true },
  { op: "not_in", label: "is none of", kinds: ["int", "float", "text"], value: true },
  { op: "has_flags", label: "has flags", kinds: ["int"], value: true },
  { op: "lacks_flags", label: "lacks flags", kinds: ["int"], value: true },
  { op: "contains", label: "contains", kinds: ["text", "int"], value: true },
  { op: "starts", label: "starts with", kinds: ["text"], value: true },
  { op: "ends", label: "ends with", kinds: ["text"], value: true },
  { op: "empty", label: "is empty / 0", kinds: ["int", "float", "bool", "text"], value: false },
  { op: "not_empty", label: "is set (not 0)", kinds: ["int", "float", "bool", "text"], value: false },
];

const newCondition = (field = ""): TaskSearchCondition => ({ field, op: "eq", value: "", sameRow: false });

/** One-click starting points; each fills in editable conditions. Hidden when the layout lacks a field. */
const TEMPLATES: { label: string; conditions: Omit<TaskSearchCondition, "sameRow">[]; hint: string }[] = [
  { label: "Rewards item…", conditions: [{ field: "any:award:candidates.items.item_id", op: "eq", value: "" }], hint: "An item among the candidate rewards of any award" },
  { label: "Requires item…", conditions: [{ field: "premise_items.item_id", op: "eq", value: "" }], hint: "Items needed to accept the quest" },
  { label: "Gives item on accept…", conditions: [{ field: "given_items.item_id", op: "eq", value: "" }], hint: "Items handed out when the quest is accepted" },
  { label: "Collect item…", conditions: [{ field: "items_wanted.item_id", op: "eq", value: "" }], hint: "Items to collect" },
  { label: "Kill monster…", conditions: [{ field: "monsters_wanted.monster_id", op: "eq", value: "" }], hint: "Monsters to kill" },
  { label: "Given by NPC…", conditions: [{ field: "fixed.deliver_npc", op: "eq", value: "" }], hint: "The NPC that hands out the quest" },
  { label: "Completed at NPC…", conditions: [{ field: "fixed.award_npc", op: "eq", value: "" }], hint: "The NPC that rewards the quest" },
  { label: "Requires class…", conditions: [{ field: "fixed.occupations", op: "eq", value: "" }], hint: "A class in the class requirement" },
  { label: "Requires quest…", conditions: [{ field: "fixed.premise_tasks", op: "eq", value: "" }], hint: "Quests to finish first" },
  { label: "Leads to quest…", conditions: [{ field: "any:award:new_task_id", op: "eq", value: "" }], hint: "The next quest an award starts" },
  { label: "Dialog option gives quest…", conditions: [{ field: "any:dialog:windows.options.parameter", op: "eq", value: "" }], hint: "A talk option whose parameter is this quest ID" },
  { label: "Quest type…", conditions: [{ field: "fixed.task_type", op: "eq", value: "" }], hint: "Career, Challenge, Romance…" },
  { label: "Level between…", conditions: [{ field: "fixed.premise_level_min", op: "ge", value: "" }, { field: "fixed.premise_level_max", op: "le", value: "" }], hint: "Minimum and maximum level to accept" },
  { label: "Name contains…", conditions: [{ field: "fixed.name", op: "contains", value: "" }], hint: "Part of the quest name" },
];

interface PickerEntry {
  key: string;
  kind: Kind;
  reference?: "task" | "element";
  tab: TaskFormTab;
  where: string;
  label: string;
  /** Lists the field sits in, relative to `key` (for "same row"). */
  lists: string[];
  /** How many fields an `any:` entry covers. */
  covers?: number;
}

/** Fields named as the form names them, plus "any award" / "any dialog" fields. */
function pickerEntries(fields: TaskSearchField[]): PickerEntry[] {
  const entries: PickerEntry[] = [];
  const anys = new Map<string, { field: TaskSearchField; covers: number }>();
  for (const field of fields) {
    const place = describeTaskField(field.path);
    entries.push({ key: field.path, kind: field.kind, reference: field.reference, tab: place.tab, where: `${place.tabLabel} › ${place.group}`, label: place.label, lists: field.arrays });
    if (field.any) {
      const known = anys.get(field.any);
      anys.set(field.any, { field: known?.field ?? field, covers: (known?.covers ?? 0) + 1 });
    }
  }
  const any: PickerEntry[] = [...anys].filter(([, entry]) => entry.covers > 1).map(([key, { field, covers }]) => {
    const place = describeTaskField(key);
    const suffix = key.replace(/^any:[a-z]+:/, "");
    const root = field.path.length - suffix.length;
    return { key, kind: field.kind, reference: field.reference, tab: place.tab, where: `${place.tabLabel} › ${place.group}`, label: place.label, lists: field.arrays.filter((list) => list.length > root).map((list) => list.slice(root)), covers };
  });
  return [...any, ...entries];
}

function FieldPicker({ value, entries, invalid, onChange }: { value: string; entries: PickerEntry[]; invalid: boolean; onChange: (key: string) => void }) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const root = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const current = entries.find((entry) => entry.key === value);
  useEffect(() => {
    if (!open) return;
    search.current?.focus();
    const outside = (event: PointerEvent) => { if (!root.current?.contains(event.target as Node)) setOpen(false); };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);
  const filtered = useMemo(() => {
    const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    return entries.filter((entry) => words.every((word) => `${entry.where} ${entry.label} ${entry.key}`.toLowerCase().includes(word))).slice(0, 150);
  }, [entries, query]);
  const choose = (key: string) => {
    onChange(key);
    setOpen(false);
    setQuery("");
  };
  return <div className="sf-field-picker" ref={root} onKeyDown={(event) => { if (open && event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setOpen(false); } }}>
    <button type="button" className={"sf-control sf-field-trigger" + (invalid ? " invalid" : "")} aria-haspopup="listbox" aria-expanded={open} onClick={() => { setQuery(""); setOpen((shown) => !shown); }} title={current ? `${current.where} › ${current.label}\n${current.key}` : value}>
      {current ? <span className="task-search-field-name truncate"><span className="muted">{current.where.split(" › ").slice(1).join(" › ")} › </span>{current.label}</span> : <span className={value ? "mono" : "muted"}>{value || "Choose a field…"}</span>}
      <ChevronDown size={14} />
    </button>
    {open && <div className="sf-field-menu task-search-field-menu">
      <label className="sf-field-search"><Search size={13} /><input ref={search} value={query} onChange={(event) => setQuery(event.target.value)} onKeyDown={(event) => { event.stopPropagation(); if (event.key === "Enter" && filtered[0]) { event.preventDefault(); choose(filtered[0].key); } }} placeholder="Search fields: monster, reward item, class…" spellCheck={false} /></label>
      <div className="sf-field-options" role="listbox" aria-label="Fields">
        {filtered.map((entry) => <button key={entry.key} type="button" role="option" aria-selected={entry.key === value} className={entry.key === value ? "selected" : ""} onClick={() => choose(entry.key)} title={entry.key}>
          <span className="task-search-option-name"><span className="truncate">{entry.label}</span><span className="muted small truncate">{entry.where}{entry.covers ? ` · ${entry.covers} places` : ""}</span></span>
          <span className="sf-field-option-meta"><span className={`sf-kind ${entry.kind === "bool" ? "int" : entry.kind}`}>{entry.reference ? (entry.reference === "task" ? "quest" : "ID") : entry.kind}</span></span>
        </button>)}
        {!filtered.length && <div className="sf-field-empty">No matching fields</div>}
      </div>
      {entries.length > filtered.length && <div className="sf-field-menu-note">Showing {filtered.length} of {count(entries.length)} fields · type to narrow the list</div>}
    </div>}
  </div>;
}

/** The value box: dropdowns for named values and flags, a text box otherwise. */
function ValueInput({ condition, entry, sets, onChange }: { condition: TaskSearchCondition; entry?: PickerEntry; sets: Record<string, Map<number, string>>; onChange: (value: string) => void }) {
  const set = taskValueSet(condition.field);
  const names = set ? sets[set.key] : undefined;
  if (entry?.kind === "bool" && (condition.op === "eq" || condition.op === "ne")) {
    return <select className="sf-control" value={condition.value} onChange={(event) => onChange(event.target.value)}><option value="">Choose…</option><option value="true">true (ticked)</option><option value="false">false</option></select>;
  }
  if (names?.size && set && !set.mask && (condition.op === "eq" || condition.op === "ne")) {
    const options = [...names.entries()].sort((left, right) => left[0] - right[0]);
    return <select className="sf-control" value={condition.value} onChange={(event) => onChange(event.target.value)}>
      <option value="">Choose…</option>
      {options.map(([value, label]) => <option key={value} value={value}>[{value}] {label}</option>)}
      {condition.value !== "" && !names.has(Number(condition.value)) && <option value={condition.value}>{condition.value}</option>}
    </select>;
  }
  if (names?.size && set?.mask && (condition.op === "has_flags" || condition.op === "lacks_flags")) {
    const current = Number(condition.value || 0) >>> 0;
    return <span className="task-search-flags">{[...names.entries()].sort((left, right) => left[0] - right[0]).map(([bit, label]) => <label key={bit} className={current & (1 << bit) ? "checked" : undefined}>
      <input type="checkbox" checked={!!(current & (1 << bit))} onChange={(event) => onChange(String(((event.target.checked ? current | (1 << bit) : current & ~(1 << bit)) >>> 0) || ""))} />{label}
    </label>)}</span>;
  }
  const placeholder = condition.op === "in" || condition.op === "not_in" ? "7, 8, 15" : entry?.reference === "element" ? "item, monster or NPC ID" : entry?.reference === "task" ? "quest ID" : entry?.kind === "text" ? "text" : "number";
  return <input className="sf-control" placeholder={placeholder} value={condition.value} onChange={(event) => onChange(event.target.value)} spellCheck={false} />;
}

interface Props {
  /** The quest selected in the list, for "Under the selected quest". */
  selection: { pack: number; root: number; path: number[]; id: number; name: string } | null;
  /** The name and ID of a top-level quest, for subquest results. */
  rootLabel: (pack: number, root: number) => string;
  onOpen: (hit: TaskSearchHit, tab: TaskFormTab | null) => void;
  onExport: (targets: TaskExportTarget[]) => void;
  onClose: () => void;
}

/** Searches every quest by conditions on its fields, or for a value in any field. */
export function TaskSearchPanel({ selection, rootLabel, onOpen, onExport, onClose }: Props) {
  const [fields, setFields] = useState<TaskSearchField[]>([]);
  const [sets, setSets] = useState<Record<string, Map<number, string>>>({});
  const [mode, setMode] = useState<"conditions" | "value">("conditions");
  const [scope, setScope] = useState<"all" | "top_level" | "under">("all");
  const [conditions, setConditions] = useState<TaskSearchCondition[]>([newCondition()]);
  const [matchAll, setMatchAll] = useState(true);
  const [value, setValue] = useState("");
  const [referencesOnly, setReferencesOnly] = useState(false);
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [report, setReport] = useState<TaskSearchResults | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<{ scanned: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [active, setActive] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [templatesOpen, setTemplatesOpen] = useState(false);
  const [under, setUnder] = useState<Props["selection"]>(null);

  useEffect(() => {
    taskSearchFields().then(setFields).catch((problem) => setError(String(problem)));
  }, []);
  const entries = useMemo(() => pickerEntries(fields), [fields]);
  const byKey = useMemo(() => new Map(entries.map((entry) => [entry.key, entry])), [entries]);
  useEffect(() => {
    const keys = [...new Set(conditions.map((condition) => taskValueSet(condition.field)?.key).filter((key): key is string => !!key && !sets[key]))];
    for (const key of keys) void loadSet(key).then((names) => setSets((current) => ({ ...current, [key]: names })));
  }, [conditions, sets]);
  useEffect(() => {
    if (!busy) return;
    const timer = window.setInterval(() => { taskSearchProgress().then(setProgress).catch(() => {}); }, 300);
    return () => window.clearInterval(timer);
  }, [busy]);

  const update = (index: number, change: Partial<TaskSearchCondition>) => setConditions((current) => current.map((condition, position) => position === index ? { ...condition, ...change } : condition));
  /** Whether two fields share a list, so their conditions can require the same row. */
  const shareList = (left: string, right: string) => {
    const a = byKey.get(left), b = byKey.get(right);
    return !!a && !!b && a.lists.some((list) => b.lists.includes(list));
  };

  const run = async () => {
    const searchScope: TaskSearchScope = scope === "under" && under ? { kind: "under", pack: under.pack, root: under.root, path: under.path } : scope === "top_level" ? { kind: "top_level" } : { kind: "all" };
    const query: TaskSearchQuery = mode === "conditions"
      ? { mode, conditions: conditions.filter((condition) => condition.field).map((condition, index, used) => ({ ...condition, sameRow: index > 0 && condition.sameRow && shareList(used[index - 1].field, condition.field) })), matchAll, scope: searchScope }
      : { mode, value, referencesOnly, caseSensitive, scope: searchScope };
    setBusy(true);
    setError(null);
    setProgress(null);
    try {
      const next = await searchTasksAdvanced(query);
      setReport(next);
      setPicked(new Set());
    } catch (problem) {
      const text = String(problem).replace(/^Error: /, "");
      if (text !== "Search cancelled") setError(text);
    } finally {
      setBusy(false);
    }
  };

  const shown = report?.hits ?? [];
  const chosen = picked.size ? shown.filter((hit) => picked.has(hitKey(hit))) : shown;
  const togglePicked = (keys: string[], on: boolean) => setPicked((current) => {
    const next = new Set(current);
    for (const key of keys) { if (on) next.add(key); else next.delete(key); }
    return next;
  });
  const copyIds = () => {
    void navigator.clipboard.writeText(chosen.map((hit) => hit.id).join("\n"));
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  };
  const open = (hit: TaskSearchHit) => {
    setActive(hitKey(hit));
    const first = hit.matches[0]?.field;
    onOpen(hit, first ? describeTaskField(plainPath(first)).tab : null);
  };
  const templates = TEMPLATES.filter((template) => template.conditions.every((condition) => byKey.has(condition.field)));

  return <section className="pane search-panel task-search-panel" onKeyDown={(event) => { if (event.key === "Enter" && !busy && !(event.target instanceof HTMLButtonElement) && !(event.target instanceof HTMLSelectElement)) void run(); }}>
    <div className="pane-head">
      <span className="pane-title"><Search size={14} /> Advanced search</span>
      <span className="spacer" />
      <button className="icon-btn small" onClick={onClose} title="Close the search" aria-label="Close"><X size={15} /></button>
    </div>

    <div className="search-form">
      <div className="sf-modes" role="tablist" aria-label="Search by">
        <button role="tab" aria-selected={mode === "conditions"} className={mode === "conditions" ? "active" : ""} onClick={() => setMode("conditions")}><ListFilter size={14} /> Conditions on fields</button>
        <button role="tab" aria-selected={mode === "value"} className={mode === "value" ? "active" : ""} onClick={() => setMode("value")}><Hash size={14} /> A value anywhere</button>
      </div>

      <label className="sf-field">
        <span className="sf-label">Look in</span>
        <select className="sf-control" value={scope} onChange={(event) => { const next = event.target.value as typeof scope; setScope(next); if (next === "under") setUnder(selection); }}>
          <option value="all">All quests and subquests</option>
          <option value="top_level">Top-level quests only</option>
          <option value="under" disabled={!selection && !under}>{under ? `Under ${under.name || "quest"} (${under.id})` : selection ? `Under the selected quest (${selection.id})` : "Under the selected quest (select one first)"}</option>
        </select>
      </label>

      {mode === "conditions" ? <div className="sf-conditions">
        <div className="task-search-templates">
          <button className="link" onClick={() => setTemplatesOpen((shown) => !shown)}><Sparkles size={12} /> Start from a common search <ChevronDown size={12} /></button>
          {templatesOpen && <div className="task-search-template-list">{templates.map((template) => <button key={template.label} className="btn small" title={template.hint} onClick={() => { setConditions(template.conditions.map((condition) => ({ ...condition, sameRow: false }))); setMatchAll(true); setTemplatesOpen(false); }}>{template.label}</button>)}</div>}
        </div>
        {conditions.map((condition, index) => {
          const entry = byKey.get(condition.field);
          const ops = OPS.filter((option) => !entry || option.kinds.includes(entry.kind));
          const needsValue = OPS.find((option) => option.op === condition.op)?.value ?? true;
          const canShareRow = index > 0 && shareList(conditions[index - 1].field, condition.field);
          return <div className="sf-cond task-search-cond" key={index}>
            <span className={"sf-join" + (index === 0 ? " first" : "")}>
              {index === 0 ? "Where" : condition.sameRow && canShareRow ? "And" : matchAll ? "And" : "Or"}
            </span>
            <div className="sf-field-cell">
              <FieldPicker value={condition.field} entries={entries} invalid={!!condition.field && fields.length > 0 && !entry} onChange={(field) => {
                const next = byKey.get(field);
                const fits = !next || OPS.find((option) => option.op === condition.op)?.kinds.includes(next.kind);
                update(index, { field, value: "", ...(fits ? {} : { op: next!.kind === "text" ? "contains" : "eq" }) });
              }} />
              {canShareRow && <label className={"task-search-same-row" + (condition.sameRow ? " on" : "")} title="Both conditions must hold on one row of the same list (for example the same reward item), not on different rows">
                <input type="checkbox" checked={condition.sameRow} onChange={(event) => update(index, { sameRow: event.target.checked })} /><Link2 size={12} /> on the same row as the condition above
              </label>}
            </div>
            <select className="sf-control" value={condition.op} onChange={(event) => update(index, { op: event.target.value as TaskSearchOp })}>
              {ops.map((option) => <option key={option.op} value={option.op}>{option.label}</option>)}
            </select>
            {needsValue ? <ValueInput condition={condition} entry={entry} sets={sets} onChange={(next) => update(index, { value: next })} /> : <span className="sf-novalue muted small">no value needed</span>}
            <button className="icon-btn small sf-remove" onClick={() => setConditions((current) => current.length > 1 ? current.filter((_, position) => position !== index) : [newCondition()])} title={conditions.length > 1 ? "Remove this condition" : "Clear this condition"} aria-label="Remove condition"><Trash2 size={14} /></button>
          </div>;
        })}
        <div className="sf-cond-actions">
          <button className="sf-add" onClick={() => setConditions((current) => [...current, newCondition()])} disabled={conditions.length >= 12}><Plus size={14} /> Add condition</button>
          {conditions.length > 1 && <div className="sf-match">
            <span className="sf-label">Match</span>
            <div className="segmented" role="radiogroup" aria-label="Match">
              <button className={matchAll ? "active" : ""} onClick={() => setMatchAll(true)} title="Quests meeting every condition">All</button>
              <button className={!matchAll ? "active" : ""} onClick={() => setMatchAll(false)} title="Quests meeting at least one condition (same-row conditions count together)">Any</button>
            </div>
          </div>}
        </div>
      </div> : <>
        <div className="sf-all-fields-note"><Search size={14} /> Numbers match number fields (IDs, amounts…); anything else is searched in names, texts and dialogs.</div>
        <label className="sf-field"><span className="sf-label">Value</span><input className="sf-control" autoFocus placeholder="e.g. 12345 or Wolf" value={value} onChange={(event) => setValue(event.target.value)} spellCheck={false} /></label>
        <div className="search-row">
          <label className="check" title="Only quest, item, monster and NPC ID fields, and dialog option parameters"><input type="checkbox" checked={referencesOnly} onChange={(event) => setReferencesOnly(event.target.checked)} /> Only ID fields</label>
          <label className="check"><input type="checkbox" checked={caseSensitive} onChange={(event) => setCaseSensitive(event.target.checked)} /> Match case</label>
        </div>
      </>}

      <div className="sf-submit">
        {busy ? <>
          <button className="btn" onClick={() => void cancelTaskSearch()}><X size={14} /> Cancel</button>
          <span className="muted small"><Loader2 size={13} className="spin" /> Searching{progress && progress.total ? ` · ${count(progress.scanned)} of ${count(progress.total)} top-level quests` : "…"}</span>
        </> : <>
          <button className="btn primary" onClick={() => void run()}><Search size={14} /> Search</button>
          <span className="muted small">or press <kbd>Enter</kbd> · includes unsaved edits</span>
        </>}
      </div>
      {error && <div className="se-problems"><CircleAlert size={13} /> {error}</div>}
    </div>

    {report && <div className="search-summary"><span><b>{count(report.total)}</b> quest{report.total === 1 ? "" : "s"} <span className="muted"> · searched {count(report.scannedRoots)} top-level quests in {(report.elapsedMs / 1000).toFixed(1)} s</span></span></div>}
    {report && report.hits.length > 0 && <div className="search-pickbar">
      <label className="check" title="Pick every result shown">
        <input type="checkbox" checked={picked.size > 0 && picked.size === shown.length} ref={(input) => { if (input) input.indeterminate = picked.size > 0 && picked.size < shown.length; }} onChange={(event) => togglePicked(shown.map(hitKey), event.target.checked)} />
        {picked.size ? <span><b>{count(picked.size)}</b> picked</span> : <span className="muted">Pick results to copy or export only those</span>}
      </label>
      {picked.size > 0 && <button className="link" onClick={() => setPicked(new Set())}>Clear</button>}
      <span className="spacer" />
      <button className="link" onClick={() => onExport(chosen.map((hit) => ({ pack: hit.pack, root: hit.root, path: hit.path })))} title={picked.size ? "Export the picked quests as task JSON" : "Export the listed quests as task JSON"}><Download size={12} /> Export{picked.size ? ` (${picked.size})` : ""}</button>
      <button className="link" onClick={copyIds} title={picked.size ? "Copy the IDs of the picked quests, one per line" : "Copy the IDs of the listed quests, one per line"}>{copied ? <Check size={12} /> : <Copy size={12} />} Copy IDs</button>
    </div>}
    {report?.truncated && <div className="search-note muted small">Showing the first {count(report.hits.length)} of {count(report.total)}. Narrow the search to see the rest.</div>}

    <div className="search-results scroll">
      {report && report.hits.length === 0 && <div className="empty-note">No quest matches.</div>}
      {shown.map((hit) => {
        const [first] = hit.matches;
        const extra = hit.matches.length - 1 + hit.more;
        const place = first ? describeTaskField(plainPath(first.field)) : null;
        return <div key={hitKey(hit)} role="button" tabIndex={0} className={"search-hit pickable task-search-hit" + (active === hitKey(hit) ? " active" : "") + (picked.has(hitKey(hit)) ? " picked" : "")} onClick={() => open(hit)} onKeyDown={(event) => { if (event.key === "Enter") { event.stopPropagation(); open(hit); } }} title={hit.matches.map((match) => `${match.field} = ${match.value}`).join("\n")}>
          <input type="checkbox" className="pick-check" checked={picked.has(hitKey(hit))} onClick={(event) => event.stopPropagation()} onChange={(event) => togglePicked([hitKey(hit)], event.target.checked)} aria-label={`Pick ${hit.name}`} />
          <span className="search-hit-main">
            <span className="truncate">{hit.name || <span className="muted">Unnamed</span>}{hit.path.length > 0 && <span className="muted small"> · subquest of {rootLabel(hit.pack, hit.root)}</span>}</span>
            {first && <span className="search-hit-match truncate">{place?.label ?? first.field} = <span className="mono">{first.value === "" ? '""' : first.value}</span>{extra > 0 && <span className="muted"> +{extra}</span>}</span>}
          </span>
          <span className="mono muted small">{hit.id}</span>
        </div>;
      })}
    </div>
  </section>;
}
