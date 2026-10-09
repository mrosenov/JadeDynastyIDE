import { useEffect, useMemo, useState, type ReactNode } from "react";
import { ArrowDown, ArrowUp, Eraser, MessageSquarePlus, Play, Plus, RotateCcw, Trash2, X } from "lucide-react";
import { taskDialogs } from "../elements/api";
import { styledLines } from "../elements/text";
import type { TaskDetail, TaskDialog, TaskDialogOption, TaskDialogWindow, TaskFieldReference, TaskFieldView } from "../elements/types";

const FUNCTION = 0x80000000;
const ROOT_PARENT = 0xffffffff;
const isFunction = (target: number) => target >= FUNCTION;
const fn = (id: number) => (FUNCTION + id) >>> 0;
/** NPC functions an option can run (SERVICE_TYPE in ExpTypes.h). */
const ACTIONS: { id: number; label: string; quest?: boolean }[] = [
  { id: 6, label: "Give quest", quest: true },
  { id: 7, label: "Complete quest", quest: true },
  { id: 8, label: "Give quest item", quest: true },
  { id: 21, label: "Give up quest", quest: true },
  { id: 0, label: "Talk", quest: true },
  { id: 17, label: "Back" },
  { id: 18, label: "Exit" },
];
const QUEST_FUNCTIONS = new Set(ACTIONS.filter((action) => action.quest).map((action) => action.id));
const SHORT = 63;

const TALKS: { key: TaskDialog["talk"]; label: string; hint: string }[] = [
  { key: "delivery", label: "Delivery", hint: "Offered by the quest NPC when the player can accept the quest." },
  { key: "unqualified", label: "Unqualified", hint: "Shown (greyed out) when the quest is visible but its requirements are not met." },
  { key: "item_delivery", label: "Item delivery", hint: "The item delivery talk; few quests use it." },
  { key: "execution", label: "Execution", hint: "Shown while the quest is in progress." },
  { key: "award", label: "Award", hint: "Shown when the quest can be completed." },
];

/** What a new talk starts with: placeholders the user edits. */
function starter(talk: TaskDialog["talk"], taskId: number, name: string): TaskDialog {
  const exit: TaskDialogOption = { target: fn(18), text: "Leave", parameter: 0 };
  const options: TaskDialogOption[] = talk === "delivery" ? [{ target: fn(6), text: "Accept", parameter: taskId }, exit]
    : talk === "award" ? [{ target: fn(7), text: "Complete", parameter: taskId }, exit]
    : [exit];
  return { talk, prompt: name.slice(0, SHORT), windows: [{ id: 1, parentId: ROOT_PARENT, text: "", options }] };
}

/** The window and every window below it. */
function subtree(dialog: TaskDialog, id: number): number[] {
  const out: number[] = [];
  const visit = (current: number) => {
    if (out.includes(current)) return;
    out.push(current);
    const window = dialog.windows.find((candidate) => candidate.id === current);
    for (const option of window?.options ?? []) if (!isFunction(option.target)) visit(option.target);
  };
  visit(id);
  return out;
}

const withWindow = (dialog: TaskDialog, id: number, change: (window: TaskDialogWindow) => TaskDialogWindow): TaskDialog => ({ ...dialog, windows: dialog.windows.map((window) => window.id === id ? change(window) : window) });

/** A text box that keeps a draft and applies it when it loses focus (Enter too, for one-line boxes). */
function DraftText({ value, multiline, maxLength, placeholder, onCommit, className }: { value: string; multiline?: boolean; maxLength?: number; placeholder?: string; onCommit: (value: string) => void; className?: string }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => { if (draft !== value) onCommit(draft); };
  return multiline
    ? <textarea className={"task-dialog-text " + (className ?? "")} value={draft} rows={Math.min(8, Math.max(2, draft.split("\n").length))} placeholder={placeholder} onChange={(event) => setDraft(event.target.value)} onBlur={commit} onKeyDown={(event) => { if (event.key === "Escape") setDraft(value); }} />
    : <input className={"task-dialog-input " + (className ?? "")} value={draft} maxLength={maxLength} placeholder={placeholder} onChange={(event) => setDraft(event.target.value)} onBlur={commit} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); if (event.key === "Escape") setDraft(value); }} />;
}

/** Plays a talk like the NPC window: options open windows, Back returns, functions end the talk. */
function TalkPreview({ dialog, onClose }: { dialog: TaskDialog; onClose: () => void }) {
  const [current, setCurrent] = useState<number | null>(dialog.windows[0]?.id ?? null);
  const [ended, setEnded] = useState<string | null>(null);
  const window = dialog.windows.find((candidate) => candidate.id === current);
  const restart = () => { setCurrent(dialog.windows[0]?.id ?? null); setEnded(null); };
  const choose = (option: TaskDialogOption) => {
    if (!isFunction(option.target)) { setCurrent(option.target); return; }
    const id = option.target - FUNCTION;
    if (id === 17) {
      const parent = window?.parentId;
      if (parent === undefined || parent === ROOT_PARENT || !dialog.windows.some((candidate) => candidate.id === parent)) setEnded("Back from the first window returns to the NPC's menu.");
      else setCurrent(parent);
      return;
    }
    const action = ACTIONS.find((entry) => entry.id === id);
    setEnded(id === 18 ? "The talk closes." : `${action?.label ?? `Function ${id}`}${action?.quest && option.parameter ? ` (quest ${option.parameter})` : ""} — the NPC runs it and the talk ends.`);
  };
  return <div className="task-dialog-preview">
    <div className="task-dialog-preview-head">
      <span className="task-dialog-preview-prompt">{dialog.prompt || <span className="muted">(no prompt)</span>}</span>
      <span className="spacer" />
      <button className="icon-btn small" onClick={restart} title="Start again"><RotateCcw size={13} /></button>
      <button className="icon-btn small" onClick={onClose} title="Close the preview"><X size={13} /></button>
    </div>
    {ended ? <div className="task-dialog-preview-end">{ended} <button className="link" onClick={restart}>Start again</button></div> : window ? <>
      <div className="task-dialog-preview-text">{styledLines(window.text).map((line, index) => <div key={index}>{line.length ? line.map((run, part) => <span key={part} style={run.colour ? { color: run.colour } : undefined}>{run.text}</span>) : " "}</div>)}</div>
      <div className="task-dialog-preview-options">{window.options.map((option, index) => <button key={index} onClick={() => choose(option)}>{styledLines(option.text)[0]?.map((run, part) => <span key={part} style={run.colour ? { color: run.colour } : undefined}>{run.text}</span>) ?? option.text}</button>)}</div>
    </> : <div className="task-dialog-preview-end">This talk has no windows.</div>}
  </div>;
}

interface Props {
  detail: TaskDetail;
  /** The form's fields by dotted path, for the quest links of option parameters. */
  fields: Map<string, TaskFieldView>;
  renderReference: (reference: TaskFieldReference) => ReactNode;
  onSave: (dialog: TaskDialog, label: string) => Promise<void>;
}

/** The task's NPC talks as editable trees: windows, their options, and the windows options open. */
export function TaskDialogEditor({ detail, fields, renderReference, onSave }: Props) {
  const [dialogs, setDialogs] = useState<TaskDialog[] | null>(null);
  const [talk, setTalk] = useState<TaskDialog["talk"]>(() => { try { return (localStorage.getItem("jdide.tasks.talk") as TaskDialog["talk"] | null) ?? "delivery"; } catch { return "delivery"; } });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState(false);
  const [saved, setSaved] = useState<TaskDialog[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    taskDialogs(detail.pack, detail.root, detail.path).then((next) => { if (!cancelled) { setDialogs(next); setSaved(next); } }).catch((problem) => { if (!cancelled) setError(String(problem)); });
    return () => { cancelled = true; };
  }, [detail]);

  const chooseTalk = (next: TaskDialog["talk"]) => {
    setTalk(next);
    setPreview(false);
    try { localStorage.setItem("jdide.tasks.talk", next); } catch { /* Remembering the talk is optional. */ }
  };
  const dialog = dialogs?.find((entry) => entry.talk === talk);
  const available = TALKS.filter((entry) => dialogs?.some((candidate) => candidate.talk === entry.key));
  const byId = useMemo(() => new Map((dialog?.windows ?? []).map((window) => [window.id, window])), [dialog]);
  const storedIndex = useMemo(() => new Map((saved?.find((entry) => entry.talk === talk)?.windows ?? []).map((window, index) => [window.id, index])), [saved, talk]);

  const save = async (next: TaskDialog, label: string) => {
    const previous = dialogs;
    setDialogs((current) => current?.map((entry) => entry.talk === next.talk ? next : entry) ?? null);
    setBusy(true);
    setError(null);
    try {
      await onSave(next, label);
    } catch (problem) {
      setDialogs(previous);
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  };

  if (!dialogs) return error ? <div className="task-field-edit-error">{error}</div> : <div className="empty-note">Loading talks…</div>;
  if (!available.length) return <div className="empty-note">This task layout has no talks.</div>;

  const removeWindows = (current: TaskDialog, root: number): TaskDialog | null => {
    const gone = subtree(current, root);
    const texts = gone.map((id) => current.windows.find((window) => window.id === id)).filter((window) => window && (window.text.trim() || window.options.length > 1)).length;
    if (texts && !window.confirm(`This removes window ${root}${gone.length > 1 ? ` and the ${gone.length - 1} window${gone.length === 2 ? "" : "s"} below it` : ""}. Continue?`)) return null;
    return { ...current, windows: current.windows.filter((window) => !gone.includes(window.id)) };
  };
  const nextId = (current: TaskDialog) => Math.max(0, ...current.windows.map((window) => window.id)) + 1;

  const setAction = (windowId: number, index: number, value: string) => {
    if (!dialog) return;
    const option = byId.get(windowId)!.options[index];
    let next: TaskDialog | null = dialog;
    if (value === "window") {
      if (!isFunction(option.target)) return;
      const id = nextId(dialog);
      next = withWindow(dialog, windowId, (window) => ({ ...window, options: window.options.map((entry, position) => position === index ? { ...entry, target: id, parameter: 0 } : entry) }));
      next = { ...next, windows: [...next.windows, { id, parentId: windowId, text: "", options: [{ target: fn(17), text: "Back", parameter: 0 }] }] };
      void save(next, "Add dialog window");
      return;
    }
    const id = Number(value);
    if (!isFunction(option.target)) next = removeWindows(dialog, option.target);
    if (!next) return;
    const parameter = QUEST_FUNCTIONS.has(id) ? (option.parameter && isFunction(option.target) && QUEST_FUNCTIONS.has(option.target - FUNCTION) ? option.parameter : detail.id) : 0;
    next = withWindow(next, windowId, (window) => ({ ...window, options: window.options.map((entry, position) => position === index ? { ...entry, target: fn(id), parameter } : entry) }));
    void save(next, "Change dialog option");
  };
  const removeOption = (windowId: number, index: number) => {
    if (!dialog) return;
    const option = byId.get(windowId)!.options[index];
    let next: TaskDialog | null = dialog;
    if (!isFunction(option.target)) next = removeWindows(dialog, option.target);
    if (!next) return;
    void save(withWindow(next, windowId, (window) => ({ ...window, options: window.options.filter((_, position) => position !== index) })), "Remove dialog option");
  };
  const moveOption = (windowId: number, index: number, delta: number) => {
    if (!dialog) return;
    void save(withWindow(dialog, windowId, (window) => {
      const options = [...window.options];
      const [moved] = options.splice(index, 1);
      options.splice(index + delta, 0, moved);
      return { ...window, options };
    }), "Move dialog option");
  };
  const setOption = (windowId: number, index: number, change: Partial<TaskDialogOption>, label: string) => {
    if (!dialog) return;
    void save(withWindow(dialog, windowId, (window) => ({ ...window, options: window.options.map((entry, position) => position === index ? { ...entry, ...change } : entry) })), label);
  };

  const renderWindow = (window: TaskDialogWindow, depth: number, seen: Set<number>): ReactNode => {
    seen.add(window.id);
    const stored = storedIndex.get(window.id);
    return <div className={"task-dialog-window" + (depth === 0 ? " root" : "")} key={window.id}>
      <div className="task-dialog-window-head"><span className="task-dialog-window-id">Window {window.id}</span>{depth === 0 && <span className="muted small">first window</span>}</div>
      <DraftText value={window.text} multiline placeholder="What the NPC says…" onCommit={(text) => dialog && void save(withWindow(dialog, window.id, (entry) => ({ ...entry, text })), "Edit dialog text")} />
      <div className="task-dialog-options">
        {window.options.map((option, index) => {
          const functionId = isFunction(option.target) ? option.target - FUNCTION : null;
          const known = functionId === null || ACTIONS.some((action) => action.id === functionId);
          const quest = functionId !== null && QUEST_FUNCTIONS.has(functionId);
          const reference = stored !== undefined ? fields.get(`dialogs.${talk}.windows[${stored}].options[${index}].parameter`) : undefined;
          const child = functionId === null ? byId.get(option.target) : undefined;
          return <div className="task-dialog-option-block" key={index}>
            <div className="task-dialog-option">
              <span className="muted">▸</span>
              <DraftText value={option.text} maxLength={SHORT} placeholder="Option text" onCommit={(text) => setOption(window.id, index, { text }, "Edit dialog option")} />
              <select className="task-form-select" value={functionId === null ? "window" : String(functionId)} disabled={busy} onChange={(event) => setAction(window.id, index, event.target.value)} title="What the option does">
                <option value="window">Opens a window</option>
                {ACTIONS.map((action) => <option key={action.id} value={action.id}>{action.label}</option>)}
                {!known && <option value={functionId!}>Function {functionId}</option>}
              </select>
              {(quest || (functionId !== null && option.parameter !== 0)) && <span className="task-dialog-parameter">
                <DraftText value={String(option.parameter)} className="mono" placeholder={quest ? "quest ID" : "parameter"} onCommit={(text) => { const value = Number(text.trim()); if (Number.isInteger(value) && value >= 0) setOption(window.id, index, { parameter: value >>> 0 }, "Edit dialog option"); }} />
                {quest && option.parameter === detail.id ? <span className="muted small">this quest</span> : quest && <button className="link small" onClick={() => setOption(window.id, index, { parameter: detail.id }, "Edit dialog option")} title={`Set to this quest (${detail.id})`}>use this quest</button>}
                {reference?.reference && Number(reference.value) === option.parameter && renderReference(reference.reference)}
              </span>}
              <span className="task-form-row-actions">
                <button className="icon-btn small" disabled={busy || index === 0} onClick={() => moveOption(window.id, index, -1)} title="Move up"><ArrowUp size={13} /></button>
                <button className="icon-btn small" disabled={busy || index === window.options.length - 1} onClick={() => moveOption(window.id, index, 1)} title="Move down"><ArrowDown size={13} /></button>
                <button className="icon-btn small danger" disabled={busy} onClick={() => removeOption(window.id, index)} title={child ? "Remove this option and the window it opens" : "Remove this option"}><Trash2 size={13} /></button>
              </span>
            </div>
            {child && !seen.has(child.id) && <div className="task-dialog-child">{renderWindow(child, depth + 1, seen)}</div>}
            {functionId === null && !child && <div className="task-field-edit-error">Opens window {option.target}, which this talk does not have.</div>}
          </div>;
        })}
        <button className="btn small task-dialog-add" disabled={busy} onClick={() => dialog && void save(withWindow(dialog, window.id, (entry) => ({ ...entry, options: [...entry.options, { target: fn(18), text: "Leave", parameter: 0 }] })), "Add dialog option")}><Plus size={13} /> Add option</button>
      </div>
    </div>;
  };

  const current = TALKS.find((entry) => entry.key === talk) ?? available[0];
  const root = dialog?.windows[0];
  const seen = new Set<number>();
  return <div className="task-dialogs">
    <div className="task-award-tabs" role="tablist">{available.map((entry) => {
      const used = !!dialogs.find((candidate) => candidate.talk === entry.key)?.windows.length;
      return <button key={entry.key} role="tab" aria-selected={entry.key === current.key} className={entry.key === current.key ? "active" : ""} onClick={() => chooseTalk(entry.key)} title={entry.hint}>{entry.label}{used && <span className="task-award-dot" title="Has a talk" />}</button>;
    })}</div>
    <div className="task-award-body">
      <div className="muted small task-dialog-hint">{current.hint}</div>
      {!dialog || !root ? <div className="task-dialog-empty">
        <span className="muted">No {current.label.toLowerCase()} talk: the NPC shows nothing for this stage.</span>
        <button className="btn small" disabled={busy} onClick={() => void save(starter(current.key, detail.id, detail.name), "Create dialog")}><MessageSquarePlus size={13} /> Create talk</button>
      </div> : <>
        <div className="task-dialog-head">
          <label className="task-dialog-prompt"><span className="task-form-label">Prompt</span><DraftText value={dialog.prompt} maxLength={SHORT} placeholder="The NPC menu entry that starts the talk" onCommit={(prompt) => void save({ ...dialog, prompt }, "Edit dialog prompt")} /></label>
          <button className={"btn small" + (preview ? " active" : "")} onClick={() => setPreview((shown) => !shown)}><Play size={13} /> Preview</button>
          <button className="btn small" disabled={busy} onClick={() => { if (window.confirm(`Remove the whole ${current.label.toLowerCase()} talk (${dialog.windows.length} window${dialog.windows.length === 1 ? "" : "s"})?`)) void save({ ...dialog, prompt: "", windows: [] }, "Clear dialog"); }}><Eraser size={13} /> Clear talk</button>
        </div>
        {preview && <TalkPreview key={JSON.stringify(dialog)} dialog={dialog} onClose={() => setPreview(false)} />}
        {renderWindow(root, 0, seen)}
        {dialog.windows.filter((entry) => !seen.has(entry.id)).map((entry) => <div key={entry.id} className="task-field-edit-error">Window {entry.id} is not opened by any option, so the game never shows it and this talk cannot be saved until it is removed. <button className="link" disabled={busy} onClick={() => void save({ ...dialog, windows: dialog.windows.filter((candidate) => candidate.id === root.id || seen.has(candidate.id)) }, "Remove unreachable dialog windows")}>Remove unreachable windows</button></div>)}
      </>}
      {error && <div className="task-field-edit-error">{error}</div>}
    </div>
  </div>;
}
