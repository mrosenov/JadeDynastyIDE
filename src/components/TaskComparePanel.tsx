import { useState, type ReactNode } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ArrowLeftRight, Check, ChevronRight, ClipboardCopy, Copy, FileDiff, FolderOpen, GitCompareArrows, Loader2, X } from "lucide-react";
import { closeTaskCompare, copyComparedTasks, openTaskCompare, taskCompare, taskCompareFields } from "../elements/api";
import type { TaskChangedTask, TaskCompareFile, TaskCompareReport, TaskEditState, TaskFieldDiff, TaskImportReport, TaskOneSided } from "../elements/types";
import { bytes as fmtBytes, count } from "../elements/format";

interface Props {
  currentPath: string;
  /** Other task sets worth offering, such as the configured client's. */
  suggestions: string[];
  /** Selects a task of the open file. */
  onOpen: (pack: number, root: number, path: number[]) => void;
  onCopied: (state: TaskEditState, report: TaskImportReport) => Promise<void>;
  onClose: () => void;
}

/** Every task index is "tasks.data": show the folders that tell them apart. */
const shortPath = (path: string) => path.split(/[\\/]/).filter(Boolean).slice(-4).join("/");
const where = (task: { pack: number; root: number; path: number[] }) => `pack ${task.pack + 1} · root ${task.root + 1}${task.path.length ? ` › ${task.path.map((index) => index + 1).join(".")}` : ""}`;
const shown = (value?: string) => value === undefined ? undefined : value.length > 120 ? `${value.slice(0, 120)}…` : value === "" ? "(empty)" : value;

/** Compares the open task set with another by task ID and copies selected fields or tasks from it. */
export function TaskComparePanel({ currentPath, suggestions, onOpen, onCopied, onClose }: Props) {
  const [report, setReport] = useState<TaskCompareReport | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Patch notes read from old to new: is the compared file the older one?
  const [otherOlder, setOtherOlder] = useState(true);
  const [query, setQuery] = useState("");
  const [openGroup, setOpenGroup] = useState<"changed" | "this" | "other" | null>("changed");
  const [openTask, setOpenTask] = useState<number | null>(null);
  const [fields, setFields] = useState<Record<number, TaskFieldDiff[] | "loading">>({});
  const [picked, setPicked] = useState<Map<number, Set<string>>>(new Map());
  const [allFields, setAllFields] = useState<Set<number>>(new Set());
  const [trees, setTrees] = useState<Set<number>>(new Set());
  const [notesCopied, setNotesCopied] = useState(false);

  const resetSelection = () => {
    setPicked(new Map());
    setAllFields(new Set());
    setTrees(new Set());
    setFields({});
    setOpenTask(null);
  };

  const start = async (path: string) => {
    setBusy("Reading and comparing every task…");
    setError(null);
    try {
      setReport(await openTaskCompare(path));
      resetSelection();
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(null);
    }
  };

  const choose = async () => {
    const picked = await openDialog({ multiple: false, directory: false, title: "Compare with tasks.data", filters: [{ name: "tasks.data", extensions: ["data"] }] });
    if (typeof picked === "string") void start(picked);
  };

  const stop = () => {
    closeTaskCompare().catch(() => {});
    setReport(null);
    resetSelection();
  };

  const loadFields = async (task: TaskChangedTask) => {
    if (fields[task.id]) return;
    setFields((current) => ({ ...current, [task.id]: "loading" }));
    try {
      const diff = await taskCompareFields(task.id);
      setFields((current) => ({ ...current, [task.id]: diff }));
    } catch (problem) {
      setFields((current) => { const next = { ...current }; delete next[task.id]; return next; });
      setError(String(problem).replace(/^Error: /, ""));
    }
  };

  const toggleTask = (task: TaskChangedTask) => {
    const next = openTask === task.id ? null : task.id;
    setOpenTask(next);
    if (next !== null) void loadFields(task);
  };

  const needle = query.trim().toLowerCase();
  const matches = (task: { id: number; name: string; otherName?: string }) => !needle || task.name.toLowerCase().includes(needle) || (task.otherName ?? "").toLowerCase().includes(needle) || String(task.id) === needle;

  const head = (
    <div className="pane-head">
      <span className="pane-title"><GitCompareArrows size={14} /> Compare tasks</span>
      <span className="spacer" />
      {report && <button className="link" onClick={stop} disabled={!!busy}>Stop comparing</button>}
      <button className="icon-btn small" onClick={onClose} title="Close (Esc)" aria-label="Close"><X size={15} /></button>
    </div>
  );

  if (!report) {
    const others = suggestions.filter((path) => path && path.toLowerCase() !== currentPath.toLowerCase());
    return (
      <section className="pane search-panel compare-panel task-compare-panel" onKeyDown={(event) => event.key === "Escape" && onClose()}>
        {head}
        <div className="compare-start">
          <FileDiff size={30} className="muted" />
          <p>Compare the open <b>tasks.data</b> with another: another version, or the server's task set against the client's. Tasks are paired by ID and compared field by field; identical task trees are skipped quickly.</p>
          <button className="btn primary" onClick={() => void choose()} disabled={!!busy}>{busy ? <Loader2 size={14} className="spin" /> : <FolderOpen size={14} />} Choose tasks.data…</button>
          {others.map((path) => <button key={path} className="link" onClick={() => void start(path)} disabled={!!busy} title={path}>Compare with {shortPath(path)}</button>)}
          {busy && <p className="muted small"><Loader2 size={13} className="spin" /> {busy} Comparing two versions decodes every task and can take a while.</p>}
          {error && <div className="se-problems">{error}</div>}
        </div>
      </section>
    );
  }

  // Before / after: added tasks exist only in the newer file.
  const [added, removed] = otherOlder ? [report.onlyThis, report.onlyOther] : [report.onlyOther, report.onlyThis];
  const [addedCount, removedCount] = otherOlder ? [report.onlyThisCount, report.onlyOtherCount] : [report.onlyOtherCount, report.onlyThisCount];
  const [older, newer] = otherOlder ? [report.other, report.this] : [report.this, report.other];
  const side = (file: TaskCompareFile, label: string) => (
    <div className="compare-file" title={file.path}>
      <span className="sf-label">{label} {file === report.this && <span className="tag">open</span>}</span>
      <span className="truncate">{shortPath(file.path)}</span>
      <span className="muted small">v{file.version} · {count(file.roots)} top-level · {count(file.tasks)} tasks · {fmtBytes(file.size)}</span>
    </div>
  );

  const copyableTrees = report.onlyOther.filter((task) => task.copyable).map((task) => task.id);
  const copyableTasks = report.changed.filter((task) => task.copyableCount > 0).map((task) => task.id);
  const selectedCount = [...picked.entries()].filter(([id]) => !allFields.has(id)).reduce((sum, [, set]) => sum + set.size, 0)
    + report.changed.filter((task) => allFields.has(task.id)).reduce((sum, task) => sum + task.copyableCount, 0)
    + trees.size;
  const everything = copyableTasks.length + copyableTrees.length > 0 && copyableTasks.every((id) => allFields.has(id)) && copyableTrees.every((id) => trees.has(id));

  const setAll = (checked: boolean) => {
    setAllFields(checked ? new Set(copyableTasks) : new Set());
    setTrees(checked ? new Set(copyableTrees) : new Set());
    if (!checked) setPicked(new Map());
  };
  const toggleIn = <T,>(set: Set<T>, value: T, checked: boolean) => {
    const next = new Set(set);
    if (checked) next.add(value);
    else next.delete(value);
    return next;
  };
  const pickField = (task: TaskChangedTask, field: string, checked: boolean, diff: TaskFieldDiff[]) => {
    // Unticking one field of a task picked as a whole keeps its other fields.
    const current = allFields.has(task.id) ? new Set(diff.filter((entry) => entry.copyable).map((entry) => entry.field)) : new Set(picked.get(task.id) ?? []);
    if (checked) current.add(field);
    else current.delete(field);
    setAllFields((old) => toggleIn(old, task.id, false));
    setPicked((old) => new Map(old).set(task.id, current));
  };

  const copySelected = async () => {
    if (!selectedCount) return;
    setBusy("Copying…");
    setError(null);
    try {
      const result = await copyComparedTasks({
        fields: [...picked.entries()].filter(([id]) => !allFields.has(id)).flatMap(([id, set]) => [...set].map((field) => ({ id, field }))),
        allFields: [...allFields],
        tasks: [...trees],
      });
      if (result.state) await onCopied(result.state, result);
      if (result.rejected) setError(`${result.rejected} task${result.rejected === 1 ? " was" : "s were"} skipped: ${result.issues.map((issue) => `${issue.id ?? ""} ${issue.message}`).slice(0, 3).join("; ")}`);
      setBusy("Comparing again…");
      setReport(await taskCompare());
      resetSelection();
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(null);
    }
  };

  const copyNotes = async () => {
    const line = (task: { id: number; name: string }) => `- ${task.id} ${task.name || "(unnamed)"}`;
    const notes = [
      `# Task changes: ${shortPath(older.path)} → ${shortPath(newer.path)}`,
      "",
      `## Added (${addedCount})`, ...added.map(line), "",
      `## Removed (${removedCount})`, ...removed.map(line), "",
      `## Changed (${report.changedCount})`, ...report.changed.map((task) => `${line(task)} (${task.fieldCount} field${task.fieldCount === 1 ? "" : "s"}${task.moved ? ", moved" : ""}${task.subtasksDiffer ? ", subquests changed" : ""})`),
    ].join("\n");
    await navigator.clipboard.writeText(notes);
    setNotesCopied(true);
    window.setTimeout(() => setNotesCopied(false), 2000);
  };

  const oneSided = (task: TaskOneSided, kind: "add" | "del") => {
    const compared = report.onlyOther.includes(task);
    const content = <>
      <span className="truncate"><span className={kind === "add" ? "diff-add" : "diff-del"}>{kind === "add" ? "+ " : "− "}</span>{task.name || <span className="muted">(unnamed)</span>}<span className="muted small"> · {where(task)}{task.path.length === 0 && task.tasks > 1 ? ` · ${count(task.tasks)} quests` : ""}</span></span>
      <span className="mono muted small">{task.id}</span>
    </>;
    if (compared) {
      return <label key={`o${task.id}`} className={"search-hit compare-rec compare-select-rec" + (!task.copyable ? " disabled" : "")} title={task.copyable ? "Add this complete task tree to the open file" : task.reason}>
        <input type="checkbox" checked={trees.has(task.id)} disabled={!task.copyable || !!busy} onChange={(event) => setTrees((old) => toggleIn(old, task.id, event.target.checked))} />
        {content}
      </label>;
    }
    return <button key={`t${task.id}`} className="search-hit compare-rec" onClick={() => onOpen(task.pack, task.root, task.path)} title="Select this task">{content}</button>;
  };

  const changed = (task: TaskChangedTask) => {
    const isOpen = openTask === task.id;
    const diff = fields[task.id];
    const taskPicked = allFields.has(task.id) ? task.copyableCount : picked.get(task.id)?.size ?? 0;
    return <div key={`c${task.id}`}>
      <div className="search-hit compare-rec">
        <input type="checkbox" className="task-compare-pick" checked={taskPicked > 0 && taskPicked === task.copyableCount} ref={(input) => { if (input) input.indeterminate = taskPicked > 0 && taskPicked < task.copyableCount; }} disabled={!task.copyableCount || !!busy} onChange={(event) => { setAllFields((old) => toggleIn(old, task.id, event.target.checked)); setPicked((old) => { const next = new Map(old); next.delete(task.id); return next; }); }} title={task.copyableCount ? "Copy every compatible changed field of this task" : "No field can be copied into the open file"} />
        <button className="compare-rec-main" onClick={() => toggleTask(task)} title="Show the changed fields">
          <ChevronRight size={12} className={"caret-icon" + (isOpen ? " open" : "")} />
          <span className="truncate"><span className="diff-mod">~ </span>{task.name || <span className="muted">(unnamed)</span>}{task.otherName !== undefined && <span className="muted small"> (other: {task.otherName || "—"})</span>}</span>
          <span className="muted small">{task.fieldCount} field{task.fieldCount === 1 ? "" : "s"}{task.moved ? " · moved" : ""}{task.subtasksDiffer ? " · subquests" : ""}</span>
        </button>
        <button className="link mono small" onClick={() => onOpen(task.pack, task.root, task.path)} title="Select in the open file">{task.id}</button>
      </div>
      {isOpen && (diff === undefined || diff === "loading" ? <div className="empty-note">Reading fields…</div> : diff.length === 0 ? <div className="empty-note">Only the task's place or subquests differ.</div> : (
        <table className="field-diff">
          <thead><tr><th className="compare-pick" /><th>Field</th><th>Before</th><th>After</th></tr></thead>
          <tbody>{diff.map((entry) => {
            const [before, after] = otherOlder ? [entry.other, entry.this] : [entry.this, entry.other];
            const checked = allFields.has(task.id) ? entry.copyable : !!picked.get(task.id)?.has(entry.field);
            return <tr key={entry.field}>
              <td className="compare-pick"><input type="checkbox" checked={checked} disabled={!entry.copyable || !!busy} onChange={(event) => pickField(task, entry.field, event.target.checked, diff)} title={entry.copyable ? "Copy this field from the compared file" : "This field is missing or has another type in the open file"} /></td>
              <td className="mono">{entry.field}</td>
              <td className="diff-old" title={before}>{shown(before) ?? <span className="muted">—</span>}</td>
              <td className="diff-new" title={after}>{shown(after) ?? <span className="muted">—</span>}</td>
            </tr>;
          })}</tbody>
        </table>
      ))}
    </div>;
  };

  const group = (key: "changed" | "this" | "other", title: ReactNode, total: number, listed: number, items: ReactNode[]) => (
    <div className="search-group" key={key}>
      <button className="search-group-head compare-head" onClick={() => setOpenGroup(openGroup === key ? null : key)}>
        <ChevronRight size={14} className={"caret-icon" + (openGroup === key || needle ? " open" : "")} />
        <span className="truncate">{title}</span>
        <span className="spacer" />
        <span className="muted small">{count(total)}</span>
      </button>
      {(openGroup === key || needle) && <>
        {items.length ? items : <div className="empty-note">{needle ? "No tasks match." : "None."}</div>}
        {total > listed && !needle && <div className="search-note muted small">Showing the first {count(listed)} of {count(total)}.</div>}
      </>}
    </div>
  );

  return (
    <section className="pane search-panel compare-panel task-compare-panel" onKeyDown={(event) => event.key === "Escape" && onClose()}>
      {head}
      <div className="problems-tools">
        <div className="compare-files">
          {side(older, "Before")}
          <button className="icon-btn small" onClick={() => setOtherOlder((value) => !value)} title="Swap which file is the older one (before → after)"><ArrowLeftRight size={15} /></button>
          {side(newer, "After")}
        </div>
        <div className="compare-totals">
          <span className="diff-add">+{count(addedCount)} added</span>
          <span className="diff-del">−{count(removedCount)} removed</span>
          <span className="diff-mod">~{count(report.changedCount)} changed</span>
          <span className="muted">{count(report.identical)} identical</span>
          <span className="spacer" />
          <button className="link" onClick={() => void copyNotes()} title="Copy the added, removed and changed tasks as Markdown">{notesCopied ? <Check size={12} /> : <ClipboardCopy size={12} />} Copy patch notes</button>
        </div>
        {!report.sameLayout && <div className="muted small">Different task versions or layouts: field values with the same path and type can be copied; whole tasks cannot.</div>}
        {report.ambiguous > 0 && <div className="muted small">{count(report.ambiguous)} ID{report.ambiguous === 1 ? " is" : "s are"} used by several tasks and {report.ambiguous === 1 ? "is" : "are"} not compared.</div>}
        <input className="sf-control" placeholder="Filter by task name or ID…" value={query} onChange={(event) => setQuery(event.target.value)} spellCheck={false} />
        {(copyableTasks.length > 0 || copyableTrees.length > 0) && <div className="compare-copy-tools">
          <label className="check" title="Every compatible changed field and every compared-only task that can be added"><input type="checkbox" checked={everything} onChange={(event) => setAll(event.target.checked)} disabled={!!busy} /> Select all copyable</label>
          <span className="spacer" />
          <button className="btn small primary" onClick={() => void copySelected()} disabled={!selectedCount || !!busy} title="Copy the selection from the compared file into the open file as one undoable edit">{busy ? <Loader2 size={12} className="spin" /> : <Copy size={12} />} Copy selected ({count(selectedCount)})</button>
        </div>}
        {busy && <div className="muted small"><Loader2 size={12} className="spin" /> {busy}</div>}
        {error && <div className="se-problems compare-copy-error">{error}</div>}
      </div>
      <div className="search-results scroll">
        {group("changed", <><span className="diff-mod">~</span> Changed tasks</>, report.changedCount, report.changed.length, report.changed.filter(matches).map(changed))}
        {group(otherOlder ? "this" : "other", <><span className="diff-add">+</span> Added in {otherOlder ? "the open file" : "the compared file"}</>, addedCount, added.length, added.filter(matches).map((task) => oneSided(task, "add")))}
        {group(otherOlder ? "other" : "this", <><span className="diff-del">−</span> Removed: only in {otherOlder ? "the compared file" : "the open file"}</>, removedCount, removed.length, removed.filter(matches).map((task) => oneSided(task, "del")))}
        <div className="problems-foot muted small">Compared in {(report.elapsedMs / 1000).toFixed(1)} s.</div>
      </div>
    </section>
  );
}
