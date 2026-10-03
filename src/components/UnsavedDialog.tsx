import { useEffect } from "react";
import { Save, TriangleAlert, X } from "lucide-react";

interface Props {
  /** What happens after: "close the app", "open another file". */
  action: string;
  fileName: string;
  changed: number;
  added: number;
  deleted: number;
  onSave: () => void;
  onDiscard: () => void;
  onCancel: () => void;
}

const plural = (n: number, word: string) => `${n.toLocaleString()} ${word}${n === 1 ? "" : "s"}`;

/** Asks what to do with edits that are not saved: save, drop or stay. */
export function UnsavedDialog({ action, fileName, changed, added, deleted, onSave, onDiscard, onCancel }: Props) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const parts = [changed && plural(changed, "changed record"), added && plural(added, "added record"), deleted && plural(deleted, "deleted record")].filter(Boolean);

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
      <div className="modal unsaved-dialog" role="alertdialog" aria-label="Unsaved changes">
        <header className="modal-head">
          <TriangleAlert size={16} className="warn-icon" />
          <h3 className="truncate">Save changes to {fileName}?</h3>
          <span className="spacer" />
          <button className="icon-btn" onClick={onCancel} aria-label="Close">
            <X size={18} />
          </button>
        </header>
        <div className="delete-body">
          <p>
            Not saved yet: <b>{parts.join(", ")}</b>. If you {action} without saving, these edits are lost.
          </p>
        </div>
        <footer className="modal-foot">
          <span className="spacer" />
          <button className="btn" onClick={onCancel}>
            Cancel
          </button>
          <button className="btn danger-outline" onClick={onDiscard}>
            Don't save
          </button>
          <button className="btn primary" onClick={onSave} autoFocus>
            <Save size={14} /> Save…
          </button>
        </footer>
      </div>
    </div>
  );
}
