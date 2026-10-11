import { Fragment, useEffect, useState } from "react";
import { ArrowRight, Brain, History, Loader2, Pencil, Redo2, RotateCcw, Save, Undo2, X } from "lucide-react";
import { aiPolicyHistory } from "../elements/api";
import type { AiEditState, AiHistoryRow } from "../elements/types";

interface Props {
  /** The edit state: the history is read again whenever it changes. */
  state: AiEditState;
  busy: boolean;
  onUndo: () => void;
  onRedo: () => void;
  onRevertAll: () => void;
  onRevert: (id: number) => Promise<void>;
  /** Opens a policy and trigger by ID. */
  onOpen: (policyId: number, triggerId: number | null) => void;
  onClose: () => void;
}

const time = (ms: number) => new Date(ms).toLocaleTimeString();
const ago = (ms: number) => {
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  return `${Math.floor(s / 3600)} h ago`;
};
const show = (value: string) => (value === "" ? "(empty)" : value.replace(/\r\n|\n/g, " ↵ "));

/** Every AI edit since the file was opened, newest first, each revertable on its own when safe. */
export function AiHistoryPanel({ state, busy, onUndo, onRedo, onRevertAll, onRevert, onOpen, onClose }: Props) {
  const [entries, setEntries] = useState<AiHistoryRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reverting, setReverting] = useState<number | null>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    aiPolicyHistory()
      .then(setEntries)
      .catch((problem) => setError(String(problem).replace(/^Error: /, "")));
  }, [state]);

  const revert = async (entry: AiHistoryRow) => {
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
    || String(entry.policyId) === needle
    || String(entry.triggerId) === needle
    || entry.fields.some((field) => field.old.toLowerCase().includes(needle) || field.new.toLowerCase().includes(needle)));
  const applied = entries?.filter((entry) => !entry.undone).length ?? 0;

  return (
    <section className="pane search-panel history-panel ai-history-panel" onKeyDown={(event) => event.key === "Escape" && onClose()}>
      <div className="pane-head">
        <span className="pane-title"><History size={14} /> AI edit history</span>
        <span className="spacer" />
        <button className="icon-btn small" onClick={onUndo} disabled={!state.canUndo || busy} title={state.undo ? `Undo ${state.undo} (Ctrl+Z)` : "Nothing to undo"}><Undo2 size={15} /></button>
        <button className="icon-btn small" onClick={onRedo} disabled={!state.canRedo || busy} title={state.redo ? `Redo ${state.redo} (Ctrl+Y)` : "Nothing to redo"}><Redo2 size={15} /></button>
        <button className="icon-btn small" onClick={onRevertAll} disabled={!state.dirty || busy} title="Revert all changes since the file was opened or saved (Undo brings them back)"><RotateCcw size={15} /></button>
        <button className="icon-btn small" onClick={onClose} title="Close (Ctrl+H or Esc)" aria-label="Close"><X size={15} /></button>
      </div>
      <div className="problems-tools">
        <input className="sf-control" placeholder="Filter by edit, policy or trigger ID, or text…" value={query} onChange={(event) => setQuery(event.target.value)} spellCheck={false} />
        <div className="muted small">
          {entries ? `${applied} edit${applied === 1 ? "" : "s"} applied` : "…"}
          {state.dirty ? " · the policies differ from the " + (state.lastSaved ? "saved" : "opened") + " file" : " · the policies match the " + (state.lastSaved ? "saved" : "opened") + " file"}
          {state.lastSaved ? ` · saved at ${time(state.lastSaved)}` : " · kept in memory until saved"}
        </div>
        {error && <div className="se-problems">{error}</div>}
      </div>
      <div className="search-results scroll">
        {entries && entries.length === 0 && <div className="empty-note">No AI edits yet.</div>}
        {entries && entries.length > 0 && shown.length === 0 && <div className="empty-note">No edits match.</div>}
        {shown.map((entry) => (
          <Fragment key={entry.id}>
            {entry.savedAt && <SavedMark at={entry.savedAt} />}
            <div className={"history-entry" + (entry.undone ? " undone" : "") + (entry.revertedAt ? " reverted" : "")}>
              <div className="history-head">
                <span className="history-icon"><Pencil size={13} /></span>
                <span className="history-label truncate" title={entry.label}>{entry.label}</span>
                {entry.undone && <span className="tag">undone</span>}
                {entry.revertedAt && <span className="tag warn" title="Put back as it was before (Ctrl+Z takes the revert back)">reverted · {time(entry.revertedAt)}</span>}
                <span className="spacer" />
                <span className="muted small" title={new Date(entry.time).toLocaleString()}>{time(entry.time)} · {ago(entry.time)}</span>
                {!entry.undone && !entry.revertedAt && (
                  <button
                    className="btn small"
                    onClick={() => void revert(entry)}
                    disabled={busy || reverting !== null || !!entry.revertBlocked}
                    title={entry.revertBlocked ?? "Put the policies this edit changed back as they were before it, keeping later edits (Ctrl+Z takes the revert back)"}
                  >
                    {reverting === entry.id ? <Loader2 size={12} className="spin" /> : <RotateCcw size={12} />} Revert
                  </button>
                )}
              </div>
              {entry.policyId !== null && (
                <div className="history-record">
                  <button className="history-record-head" onClick={() => onOpen(entry.policyId!, entry.triggerId)} title="Open this policy">
                    <span className="find-icon"><Brain size={13} /></span>
                    <span className="truncate">Policy <span className="mono">{entry.policyId}</span>{entry.triggerId !== null && <>, trigger <span className="mono">{entry.triggerId}</span></>}</span>
                  </button>
                  {entry.fields.length > 0 && (
                    <div className="history-fields">
                      {entry.fields.map((field) => (
                        <button key={field.field} className="hf-row" onClick={() => onOpen(entry.policyId!, entry.triggerId)} title={`${field.field}: ${show(field.old)} → ${show(field.new)}`}>
                          <span className="hf-field mono">{field.field}</span>
                          <span className="hf-old">{show(field.old)}</span>
                          <ArrowRight size={12} className="hf-arrow" />
                          <span className="hf-new">{show(field.new)}</span>
                        </button>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </div>
          </Fragment>
        ))}
        {/* Saved before any listed edit: every edit is after it. */}
        {state.lastSaved && entries && entries.length > 0 && !entries.some((entry) => entry.savedAt) && !query && <SavedMark at={state.lastSaved} />}
      </div>
    </section>
  );
}

/** Where the file was saved: edits above it are not in the saved file. */
function SavedMark({ at }: { at: number }) {
  const when = new Date(at);
  return (
    <div className="history-saved" title={`aipolicy.data was saved at ${when.toLocaleString()}. Edits above this line are not in the saved file.`}>
      <Save size={12} /> Saved · {when.toLocaleTimeString()}
    </div>
  );
}
