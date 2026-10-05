import { Fragment, useEffect, useState } from "react";
import { ArrowRight, History, Loader2, Pencil, Redo2, RotateCcw, Save, Undo2, X } from "lucide-react";
import { editHistory, revertHistoryEntry } from "../elements/api";
import type { EditState, HistoryEntry, HistoryRecord, ListSummary } from "../elements/types";

interface Props {
  lists: ListSummary[];
  /** The edit state: the history is read again whenever it changes. */
  edits: EditState;
  icon?: (pathId?: number | null) => string | undefined;
  onUndo: () => void;
  onRedo: () => void;
  /** After a revert from the history. */
  onChanged: (next: EditState) => void;
  /** Opens a record at a field. */
  onOpen: (list: number, row: number, off: number | null, newTab: boolean) => void;
  onClose: () => void;
}

const time = (ms: number) => new Date(ms).toLocaleTimeString();
const ago = (ms: number) => {
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  return `${Math.floor(s / 3600)} h ago`;
};
/** Reverts from the history, and Revert record / all changes. */
const isRevert = (e: HistoryEntry) => e.reverts !== undefined || e.label.startsWith("Revert");
const show = (v: string) => (v === "" ? "(empty)" : v.replace(/\r\n/g, " ↵ "));

/** Every edit since the file was opened, newest first, each revertable on its own. */
export function HistoryPanel({ lists, edits, icon, onUndo, onRedo, onChanged, onOpen, onClose }: Props) {
  const [entries, setEntries] = useState<HistoryEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<number | null>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    editHistory()
      .then(setEntries)
      .catch((e) => setError(String(e)));
  }, [edits]);

  const revert = async (e: HistoryEntry) => {
    setBusy(e.id);
    setError(null);
    try {
      onChanged(await revertHistoryEntry(e.id, false));
    } catch (err) {
      const message = String(err).replace(/^Error: /, "");
      if (message.startsWith("CONFLICT:")) {
        const what = message.slice("CONFLICT:".length).trim();
        if (window.confirm(`Reverting “${e.label}”: ${what}. Revert anyway and overwrite those later edits?`)) {
          try {
            onChanged(await revertHistoryEntry(e.id, true));
          } catch (again) {
            setError(String(again));
          }
        }
      } else {
        setError(message);
      }
    } finally {
      setBusy(null);
    }
  };

  const q = query.trim().toLowerCase();
  const shown = (entries ?? []).filter(
    (e) =>
      !q ||
      e.label.toLowerCase().includes(q) ||
      e.records.some((r) => r.name.toLowerCase().includes(q) || String(r.id) === q || r.fields.some((f) => f.field.toLowerCase().includes(q))),
  );

  const record = (e: HistoryEntry, r: HistoryRecord) => (
    <div key={`${r.list}:${r.id}:${r.action}`} className="history-record">
      <button
        className="history-record-head"
        onClick={(ev) => r.row !== undefined && onOpen(r.list, r.row, r.fields[0]?.off ?? null, ev.ctrlKey || ev.metaKey)}
        disabled={r.row === undefined}
        title={r.row === undefined ? "The record is deleted" : "Open the record (Ctrl+click: new tab)"}
      >
        <span className="find-icon">{icon?.(r.icon) ? <img src={icon(r.icon)} alt="" draggable={false} /> : null}</span>
        <span className="truncate">
          {r.name || <span className="muted">#{r.row}</span>}
          <span className="muted small"> · {lists[r.list]?.name ?? `List ${r.list}`}</span>
          {r.action === "clone" && <span className="tag ok"> cloned</span>}
          {r.action === "import" && <span className="tag ok"> imported</span>}
          {r.action === "copy" && <span className="tag ok"> copied</span>}
          {r.action === "delete" && <span className="tag danger"> deleted</span>}
        </span>
        <span className="mono muted small">{r.id}</span>
      </button>
      {r.fields.length > 0 && (
        <div className="history-fields">
          {r.fields.map((f) => (
            <button
              key={`${f.off}:${f.field}`}
              className="hf-row"
              onClick={(ev) => r.row !== undefined && onOpen(r.list, r.row, f.off, ev.ctrlKey || ev.metaKey)}
              title={`${f.field}: ${show(f.old)} → ${show(f.new)}\nOpen the record at this field`}
            >
              <span className="hf-field mono">{f.field}</span>
              <span className="hf-old">{show(f.old)}</span>
              <ArrowRight size={12} className="hf-arrow" />
              <span className="hf-new">{show(f.new)}</span>
            </button>
          ))}
        </div>
      )}
      {r.fields.length === 0 && e.undone && <div className="muted small history-none">Undone</div>}
    </div>
  );

  return (
    <section className="pane search-panel history-panel" onKeyDown={(ev) => ev.key === "Escape" && onClose()}>
      <div className="pane-head">
        <span className="pane-title">
          <History size={14} /> Edit history
        </span>
        <span className="spacer" />
        <button className="icon-btn small" onClick={onUndo} disabled={!edits.undo} title={edits.undo ? `Undo ${edits.undo} (Ctrl+Z)` : "Nothing to undo"}>
          <Undo2 size={15} />
        </button>
        <button className="icon-btn small" onClick={onRedo} disabled={!edits.redo} title={edits.redo ? `Redo ${edits.redo} (Ctrl+Y)` : "Nothing to redo"}>
          <Redo2 size={15} />
        </button>
        <button className="icon-btn small" onClick={onClose} title="Close (Esc)" aria-label="Close">
          <X size={15} />
        </button>
      </div>
      <div className="problems-tools">
        <input className="sf-control" placeholder="Filter by edit, record, ID or field…" value={query} onChange={(e) => setQuery(e.target.value)} spellCheck={false} />
        <div className="muted small">
          {entries ? `${entries.filter((e) => !e.undone).length} edit(s) applied` : "…"}
          {edits.changed.length > 0 && ` · ${edits.changed.length} record(s) differ from the ${edits.lastSaved ? "saved" : "opened"} file`}
          {edits.changedTalks.length > 0 && ` · ${edits.changedTalks.length} dialog translation(s) differ`}
          {edits.lastSaved ? ` · saved at ${new Date(edits.lastSaved * 1000).toLocaleTimeString()}` : " · kept in memory until saved"}
        </div>
        {error && <div className="se-problems">{error}</div>}
      </div>
      <div className="search-results scroll">
        {entries && entries.length === 0 && <div className="empty-note">No edits yet. Double-click a value in the inspector to edit it.</div>}
        {entries && entries.length > 0 && shown.length === 0 && <div className="empty-note">No edits match.</div>}
        {shown.map((e) => (
          <Fragment key={e.id}>
            {e.savedAt && <SavedMark at={e.savedAt} />}
            <div className={"history-entry" + (e.undone ? " undone" : "") + (e.revertedBy ? " reverted" : "")}>
              <div className="history-head">
                <span className={"history-icon" + (isRevert(e) ? " revert" : "")}>{isRevert(e) ? <RotateCcw size={13} /> : <Pencil size={13} />}</span>
                <span className="history-label truncate" title={e.label}>
                  {e.label}
                </span>
                {e.undone && <span className="tag">undone</span>}
                {e.revertedBy && (
                  <span className="tag warn" title="Reverted to the value it had before (Ctrl+Z takes the revert back)">
                    reverted{e.revertedAt ? ` · ${time(e.revertedAt)}` : ""}
                  </span>
                )}
                <span className="spacer" />
                <span className="muted small" title={new Date(e.time).toLocaleString()}>
                  {time(e.time)} · {ago(e.time)}
                </span>
                {!e.undone && !e.revertedBy && (
                  <button
                    className="btn small"
                    onClick={() => revert(e)}
                    disabled={busy !== null}
                    title="Put the fields back to the values they had before this edit (Ctrl+Z takes the revert back)"
                  >
                    {busy === e.id ? <Loader2 size={12} className="spin" /> : <RotateCcw size={12} />} Revert
                  </button>
                )}
              </div>
              {e.records.map((r) => record(e, r))}
            </div>
          </Fragment>
        ))}
        {/* Saved before any listed edit: every edit is after it. */}
        {edits.lastSaved && entries && entries.length > 0 && !entries.some((e) => e.savedAt) && !query && <SavedMark at={edits.lastSaved} />}
      </div>
    </section>
  );
}

/** Where the file was saved: edits above it are not in the saved file. */
function SavedMark({ at }: { at: number }) {
  const when = new Date(at * 1000);
  return (
    <div className="history-saved" title={`The file was saved at ${when.toLocaleString()}. Edits above this line are not in the saved file.`}>
      <Save size={12} /> Saved · {when.toLocaleTimeString()}
    </div>
  );
}
