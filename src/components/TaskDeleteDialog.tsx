import { useEffect } from "react";
import { AlertTriangle, Check, Loader2, Trash2, X } from "lucide-react";
import type { TaskDeletePreview } from "../elements/types";

interface Props {
  preview: TaskDeletePreview;
  busy: boolean;
  error: string | null;
  onConfirm: () => void;
  onClose: () => void;
}

export function TaskDeleteDialog({ preview, busy, error, onConfirm, onClose }: Props) {
  const referenced = preview.referenceCount > 0;

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
      <header className="modal-head"><Trash2 size={18} className="danger-icon"/><div><h3 id="task-delete-title">Delete subquest subtree?</h3><p className="muted small">Undo can bring the subtree back.</p></div><span className="spacer"/><button className="icon-btn" onClick={onClose} disabled={busy} aria-label="Close"><X size={18}/></button></header>
      <div className="task-delete-summary">
        <div><span>Selected quest</span><b>{preview.id} · {preview.name || "(unnamed task)"}</b></div>
        <div><span>Will remove</span><b>{preview.tasks} quest{preview.tasks === 1 ? "" : "s"}</b></div>
      </div>
      {referenced ? <section className="task-delete-warning"><header><AlertTriangle size={16}/><div><b>{preview.referenceCount} surviving task reference{preview.referenceCount === 1 ? "" : "s"} will become unresolved</b><span>JD IDE will not rewrite these references automatically.</span></div></header><div className="task-delete-reference-head"><span>Source quest</span><span>Field</span><span>Removed ID</span></div>{preview.references.map((reference, index) => <div className="task-delete-reference" key={`${reference.pack}:${reference.root}:${reference.path.join(".")}:${reference.field}:${index}`}><span><b>{reference.sourceId}</b> · {reference.sourceName || "(unnamed task)"}</span><span className="mono truncate" title={reference.field}>{reference.field}</span><span className="mono">{reference.targetId}</span></div>)}{preview.referencesTruncated && <footer>Showing the first {preview.references.length} references.</footer>}</section>
        : <div className="task-delete-safe"><Check size={15}/><span>No surviving task reference points to an ID in this subtree.</span></div>}
      {error && <div className="path-data-message error" role="alert">{error}</div>}
      <footer className="modal-foot"><span className="muted small">The complete subtree is removed as one root edit.</span><span className="spacer"/><button className="btn" onClick={onClose} disabled={busy}>Cancel</button><button className="btn danger-solid" onClick={onConfirm} disabled={busy}>{busy ? <Loader2 size={14} className="spin"/> : <Trash2 size={14}/>} {busy ? "Checking and deleting…" : referenced ? "Delete despite references" : "Delete subtree"}</button></footer>
    </div>
  </div>;
}
