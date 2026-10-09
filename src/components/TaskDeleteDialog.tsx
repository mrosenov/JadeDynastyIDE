import { useEffect, useState } from "react";
import { AlertTriangle, Check, Loader2, Trash2, X } from "lucide-react";
import type { TaskDeletePreview } from "../elements/types";

interface Props {
  preview: TaskDeletePreview;
  busy: boolean;
  error: string | null;
  /** With the elements.data places (list, row, offset, quest ID) to clear. */
  onConfirm: (places: [number, number, number, number][]) => void;
  onClose: () => void;
}

export function TaskDeleteDialog({ preview, busy, error, onConfirm, onClose }: Props) {
  const referenced = preview.referenceCount > 0;
  const topLevel = preview.path.length === 0;
  const [ticked, setTicked] = useState<Set<number>>(() => new Set(preview.elementUses.map((_, index) => index)));
  const places = preview.elementUses.filter((_, index) => ticked.has(index)).map((use) => [use.list, use.row, use.off, use.taskId] as [number, number, number, number]);
  const elementsFile = preview.elementsPath?.split(/[\\/]/).pop();

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || busy) return;
      event.stopImmediatePropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [busy, onClose]);
  return <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !busy && onClose()}>
    <div className="modal task-delete-dialog" role="alertdialog" aria-labelledby="task-delete-title">
      <header className="modal-head"><Trash2 size={18} className="danger-icon"/><div><h3 id="task-delete-title">{topLevel ? "Delete top-level quest?" : "Delete subquest subtree?"}</h3><p className="muted small">{topLevel ? "Undo can bring it back until you save; saving removes it from its pack." : "Undo can bring the subtree back."}</p></div><span className="spacer"/><button className="icon-btn" onClick={onClose} disabled={busy} aria-label="Close"><X size={18}/></button></header>
      <div className="task-delete-summary">
        <div><span>Selected quest</span><b>{preview.id} · {preview.name || "(unnamed task)"}</b></div>
        <div><span>Will remove</span><b>{preview.tasks} quest{preview.tasks === 1 ? "" : "s"}</b></div>
      </div>
      {referenced ? <section className="task-delete-warning"><header><AlertTriangle size={16}/><div><b>{preview.referenceCount} surviving task reference{preview.referenceCount === 1 ? "" : "s"} will become unresolved</b><span>JD IDE will not rewrite these references automatically.</span></div></header><div className="task-delete-reference-head"><span>Source quest</span><span>Field</span><span>Removed ID</span></div>{preview.references.map((reference, index) => <div className="task-delete-reference" key={`${reference.pack}:${reference.root}:${reference.path.join(".")}:${reference.field}:${index}`}><span><b>{reference.sourceId}</b> · {reference.sourceName || "(unnamed task)"}</span><span className="mono truncate" title={reference.field}>{reference.field}</span><span className="mono">{reference.targetId}</span></div>)}{preview.referencesTruncated && <footer>Showing the first {preview.references.length} references.</footer>}</section>
        : <div className="task-delete-safe"><Check size={15}/><span>No surviving task reference points to an ID in this {topLevel ? "quest" : "subtree"}.</span></div>}
      {preview.elementsPath && (preview.elementUses.length ? <section className="task-id-section task-delete-elements">
        <label className="check task-id-all"><input type="checkbox" checked={ticked.size === preview.elementUses.length} ref={(input) => { if (input) input.indeterminate = ticked.size > 0 && ticked.size < preview.elementUses.length; }} onChange={(event) => setTicked(new Set(event.target.checked ? preview.elementUses.map((_, index) => index) : []))} disabled={busy} /> Clear {ticked.size} of {preview.elementUses.length} place{preview.elementUses.length === 1 ? "" : "s"} in {elementsFile} that name the deleted quest{preview.lostIds.length === 1 ? "" : "s"} (set to 0, which NPC quest lists skip; one undo step there)</label>
        <div className="task-id-list">{preview.elementUses.map((use, index) => <label key={index} className="task-id-use">
          <input type="checkbox" checked={ticked.has(index)} disabled={busy} onChange={(event) => setTicked((current) => { const next = new Set(current); if (event.target.checked) next.add(index); else next.delete(index); return next; })} />
          <span><b>{use.id}</b> · {use.name || "(unnamed)"}</span><span className="muted truncate">{use.listName} › <span className="mono">{use.field}</span> = {use.taskId}</span>
        </label>)}</div>
      </section> : <div className="task-delete-safe"><Check size={15}/><span>No quest-ID field of {elementsFile} names the deleted quest{preview.lostIds.length === 1 ? "" : "s"}.</span></div>)}
      {error && <div className="path-data-message error" role="alert">{error}</div>}
      <footer className="modal-foot"><span className="muted small">{topLevel ? "The quest and its subquests are removed as one edit." : "The complete subtree is removed as one root edit."}</span><span className="spacer"/><button className="btn" onClick={onClose} disabled={busy}>Cancel</button><button className="btn danger-solid" onClick={() => onConfirm(places)} disabled={busy}>{busy ? <Loader2 size={14} className="spin"/> : <Trash2 size={14}/>} {busy ? "Checking and deleting…" : referenced ? "Delete despite references" : topLevel ? "Delete quest" : "Delete subtree"}</button></footer>
    </div>
  </div>;
}
