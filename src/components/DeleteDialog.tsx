import { useEffect, useState } from "react";
import { Link2, Loader2, Trash2, TriangleAlert, X } from "lucide-react";
import { referencedBy } from "../elements/api";
import type { ListSummary, ReferencedBy } from "../elements/types";

interface Props {
  list: ListSummary;
  row: number;
  name: string;
  id: number;
  lists: ListSummary[];
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * Asks before deleting a record, listing the records whose fields point at
 * its ID (they would point at nothing afterwards).
 */
export function DeleteDialog({ list, row, name, id, lists, onConfirm, onCancel }: Props) {
  const [refs, setRefs] = useState<ReferencedBy | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    referencedBy(list.index, row)
      .then(setRefs)
      .catch((e) => setError(String(e)));
  }, [list.index, row]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const referrers = refs?.referrers ?? [];
  const broken = referrers.length > 0;

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
      <div className="modal delete-dialog" role="alertdialog" aria-label="Delete record">
        <header className="modal-head">
          <Trash2 size={16} className="danger-icon" />
          <h3 className="truncate">Delete {name || `record #${row}`}?</h3>
          <span className="spacer" />
          <button className="icon-btn" onClick={onCancel} aria-label="Close">
            <X size={18} />
          </button>
        </header>
        <div className="delete-body">
          <p>
            <b>{name || `#${row}`}</b> <span className="muted">(ID {id})</span> is removed from <b>{list.name}</b>. Undo (Ctrl+Z) or the edit history brings it back.
          </p>
          {!refs && !error && (
            <div className="muted small">
              <Loader2 size={13} className="spin" /> Looking for records that point at ID {id}…
            </div>
          )}
          {error && <div className="se-problems">{error}</div>}
          {refs && !broken && (
            <div className="delete-ok small">
              <Link2 size={13} /> No record of this file points at ID {id}.
            </div>
          )}
          {broken && (
            <div className="delete-warning">
              <div className="delete-warning-head">
                <TriangleAlert size={15} /> {referrers.length}
                {refs!.truncated ? "+" : ""} record{referrers.length === 1 ? "" : "s"} point at ID {id}. After deleting, they point at nothing.
              </div>
              <ul>
                {referrers.slice(0, 8).map((r, i) => (
                  <li key={i}>
                    <span className="muted">{lists[r.list]?.name ?? `List ${r.list}`} ›</span> {r.name || `#${r.row}`}{" "}
                    <span className="mono muted small">· {r.field}</span>
                    {r.how === "id" && <span className="muted small" title="Matched by field name and ID space, not a declared reference"> (likely)</span>}
                  </li>
                ))}
              </ul>
              {referrers.length > 8 && <div className="muted small">and {referrers.length - 8} more (see Referenced by in the inspector)</div>}
            </div>
          )}
        </div>
        <footer className="modal-foot">
          <span className="spacer" />
          <button className="btn" onClick={onCancel} autoFocus>
            Cancel
          </button>
          <button className="btn danger-solid" onClick={onConfirm} disabled={!refs && !error}>
            <Trash2 size={14} /> {broken ? "Delete anyway" : "Delete"}
          </button>
        </footer>
      </div>
    </div>
  );
}
