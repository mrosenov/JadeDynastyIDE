import { Fragment, useEffect, useState } from "react";
import { ArrowRight, History, ListTree, Loader2, Pencil, Redo2, RotateCcw, Save, Undo2, X } from "lucide-react";
import { getTaskEditHistory } from "../elements/api";
import type { TaskEditState, TaskHistoryEntry } from "../elements/types";

interface Props {
  /** The edit state: the history is read again whenever it changes. */
  edits: TaskEditState;
  /** An edit, undo or revert is running. */
  busy: boolean;
  onUndo: () => void;
  onRedo: () => void;
  onRevertAll: () => void;
  onRevert: (id: number) => Promise<void>;
  /** Selects a quest by ID in the task list. */
  onOpen: (taskId: number) => void;
  onClose: () => void;
}

const time = (ms: number) => new Date(ms).toLocaleTimeString();
const ago = (ms: number) => {
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  return `${Math.floor(s / 3600)} h ago`;
};
const show = (value: string) => (value === "" ? "(empty)" : value.replace(/\r\n/g, " ↵ "));

/** Every task edit since the task set was opened, newest first, each revertable on its own when safe. */
export function TaskHistoryPanel({ edits, busy, onUndo, onRedo, onRevertAll, onRevert, onOpen, onClose }: Props) {
  const [entries, setEntries] = useState<TaskHistoryEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reverting, setReverting] = useState<number | null>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    getTaskEditHistory()
      .then(setEntries)
      .catch((problem) => setError(String(problem).replace(/^Error: /, "")));
  }, [edits]);

  const revert = async (entry: TaskHistoryEntry) => {
    setReverting(entry.id);
    try {
      await onRevert(entry.id);
    } finally {
      setReverting(null);
    }
  };

  const needle = query.trim().toLowerCase();
  const shown = (entries ?? []).filter((entry) =>
    !needle
    || entry.label.toLowerCase().includes(needle)
    || entry.taskName.toLowerCase().includes(needle)
    || String(entry.taskId) === needle
    || entry.field.toLowerCase().includes(needle));
  const applied = entries?.filter((entry) => !entry.undone).length ?? 0;

  return (
    <section className="pane search-panel history-panel task-history-panel" onKeyDown={(event) => event.key === "Escape" && onClose()}>
      <div className="pane-head">
        <span className="pane-title"><History size={14} /> Task edit history</span>
        <span className="spacer" />
        <button className="icon-btn small" onClick={onUndo} disabled={!edits.undo || busy} title={edits.undo ? `Undo ${edits.undo} (Ctrl+Z)` : "Nothing to undo"}><Undo2 size={15} /></button>
        <button className="icon-btn small" onClick={onRedo} disabled={!edits.redo || busy} title={edits.redo ? `Redo ${edits.redo} (Ctrl+Y)` : "Nothing to redo"}><Redo2 size={15} /></button>
        <button className="icon-btn small" onClick={onRevertAll} disabled={!edits.changedRoots.length || busy} title="Revert all task edits (Undo brings them back)"><RotateCcw size={15} /></button>
        <button className="icon-btn small" onClick={onClose} title="Close (Esc)" aria-label="Close"><X size={15} /></button>
      </div>
      <div className="problems-tools">
        <input className="sf-control" placeholder="Filter by edit, quest name, task ID or field…" value={query} onChange={(event) => setQuery(event.target.value)} spellCheck={false} />
        <div className="muted small">
          {entries ? `${applied} edit${applied === 1 ? "" : "s"} applied` : "…"}
          {edits.changedRoots.length > 0 && ` · ${edits.changedRoots.length} task root${edits.changedRoots.length === 1 ? "" : "s"} differ from the ${edits.lastSaved ? "saved" : "opened"} task set`}
          {edits.lastSaved ? ` · saved at ${time(edits.lastSaved)}` : " · kept in memory until saved"}
        </div>
        {error && <div className="se-problems">{error}</div>}
      </div>
      <div className="search-results scroll">
        {entries && entries.length === 0 && <div className="empty-note">No task edits yet. Click a value in the inspector to edit it.</div>}
        {entries && entries.length > 0 && shown.length === 0 && <div className="empty-note">No edits match.</div>}
        {shown.map((entry) => (
          <Fragment key={entry.id}>
            {entry.savedAt && <SavedMark at={entry.savedAt} />}
            <div className={"history-entry" + (entry.undone ? " undone" : "") + (entry.revertedAt ? " reverted" : "")}>
              <div className="history-head">
                <span className="history-icon"><Pencil size={13} /></span>
                <span className="history-label truncate" title={entry.label}>{entry.label}</span>
                {entry.undone && <span className="tag">undone</span>}
                {entry.revertedAt && <span className="tag warn" title="Put back to the bytes it had before (Ctrl+Z takes the revert back)">reverted · {time(entry.revertedAt)}</span>}
                <span className="spacer" />
                <span className="muted small" title={new Date(entry.time).toLocaleString()}>{time(entry.time)} · {ago(entry.time)}</span>
                {!entry.undone && !entry.revertedAt && (
                  <button
                    className="btn small"
                    onClick={() => void revert(entry)}
                    disabled={busy || reverting !== null || !!entry.revertBlocked}
                    title={entry.revertBlocked ?? "Put the changed task roots back as they were before this edit, keeping later edits (Ctrl+Z takes the revert back)"}
                  >
                    {reverting === entry.id ? <Loader2 size={12} className="spin" /> : <RotateCcw size={12} />} Revert
                  </button>
                )}
              </div>
              <div className="history-record">
                <button className="history-record-head" onClick={() => entry.taskId && onOpen(entry.taskId)} disabled={!entry.taskId} title={entry.taskId ? "Select this quest" : undefined}>
                  <span className="find-icon"><ListTree size={13} /></span>
                  <span className="truncate">{entry.taskName || <span className="muted">(unnamed task)</span>}</span>
                  {!!entry.taskId && <span className="mono muted small">{entry.taskId}</span>}
                </button>
                {(entry.old || entry.new) && (
                  <div className="history-fields">
                    <button className="hf-row" onClick={() => entry.taskId && onOpen(entry.taskId)} title={`${entry.field}: ${show(entry.old)} → ${show(entry.new)}`}>
                      <span className="hf-field mono">{entry.field}</span>
                      <span className="hf-old">{show(entry.old)}</span>
                      <ArrowRight size={12} className="hf-arrow" />
                      <span className="hf-new">{show(entry.new)}</span>
                    </button>
                  </div>
                )}
              </div>
            </div>
          </Fragment>
        ))}
        {/* Saved before any listed edit: every edit is after it. */}
        {edits.lastSaved && entries && entries.length > 0 && !entries.some((entry) => entry.savedAt) && !query && <SavedMark at={edits.lastSaved} />}
      </div>
    </section>
  );
}

/** Where the task set was saved: edits above it are not in the saved files. */
function SavedMark({ at }: { at: number }) {
  const when = new Date(at);
  return (
    <div className="history-saved" title={`The task set was saved at ${when.toLocaleString()}. Edits above this line are not in the saved files.`}>
      <Save size={12} /> Saved · {when.toLocaleTimeString()}
    </div>
  );
}
