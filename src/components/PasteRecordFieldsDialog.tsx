import { useEffect, useMemo, useState } from "react";
import { Check, ClipboardPaste, CircleAlert, X } from "lucide-react";
import type { FieldEdit, FieldNode } from "../elements/types";
import { nodeAt, type Path } from "../elements/fieldPaths";
import { isEditable } from "../elements/edit";

export interface CopiedField {
  path: Path;
  name: string;
  ty: string;
  value: string;
}

export interface FieldClipboard {
  list: number;
  listName: string;
  sourceRow: number;
  sourceId: number;
  fields: CopiedField[];
}

interface Props {
  clipboard: FieldClipboard;
  targetRow: number;
  targetId: number;
  nodes: FieldNode[];
  onApply: (edits: FieldEdit[], label: string) => Promise<string | null>;
  onClose: () => void;
}

interface PasteRow {
  source: CopiedField;
  target: FieldNode | null;
  status: "change" | "same" | "missing" | "type";
}

export function PasteRecordFieldsDialog({ clipboard, targetRow, targetId, nodes, onApply, onClose }: Props) {
  const rows = useMemo<PasteRow[]>(
    () => clipboard.fields.map((source) => {
      const target = nodeAt(nodes, source.path);
      if (!target || !isEditable(target)) return { source, target: null, status: "missing" };
      if (target.ty !== source.ty) return { source, target, status: "type" };
      return { source, target, status: target.value === source.value ? "same" : "change" };
    }),
    [clipboard, nodes],
  );
  const available = rows.filter((row) => row.status === "change");
  const [selected, setSelected] = useState<Set<Path>>(() => new Set(available.map((row) => row.source.path)));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [busy, onClose]);

  const apply = async () => {
    const chosen = available.filter((row) => selected.has(row.source.path));
    if (!chosen.length || busy) return;
    setBusy(true);
    setError(null);
    const problem = await onApply(
      chosen.map((row) => ({ off: row.target!.off, value: row.source.value })),
      `Paste ${chosen.length} field${chosen.length === 1 ? "" : "s"} from ID ${clipboard.sourceId}`,
    );
    setBusy(false);
    if (problem) setError(problem.replace(/^Error: /, ""));
    else onClose();
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !busy && onClose()}>
      <div className="modal paste-fields-dialog" role="dialog" aria-modal="true" aria-label="Paste selected fields">
        <header className="modal-head">
          <ClipboardPaste size={16} /><h3>Paste selected fields</h3><span className="spacer" />
          <button className="icon-btn" onClick={onClose} disabled={busy} aria-label="Close"><X size={18} /></button>
        </header>
        <div className="paste-fields-summary">
          <span><b>{clipboard.listName}</b></span>
          <span className="muted">ID {clipboard.sourceId} → ID {targetId}</span>
          <span className="muted small">Fields match by schema path and exact runtime type. Record IDs are never copied.</span>
        </div>
        {error && <div className="se-problems" role="alert"><CircleAlert size={14} /> {error}</div>}
        <div className="paste-fields-list scroll">
          <div className="paste-fields-row table-head"><span /><span>Field</span><span>Current</span><span>Copied</span><span>Status</span></div>
          {rows.map((row) => {
            const usable = row.status === "change";
            return (
              <div className={"paste-fields-row" + (selected.has(row.source.path) ? " picked" : "")} key={row.source.path}>
                <span>{usable && <input type="checkbox" checked={selected.has(row.source.path)} onChange={(event) => setSelected((current) => {
                  const next = new Set(current);
                  if (event.target.checked) next.add(row.source.path); else next.delete(row.source.path);
                  return next;
                })} />}</span>
                <span className="mono truncate" title={row.source.path}>{row.source.path}</span>
                <span className="truncate">{row.target?.value || <span className="muted">(empty)</span>}</span>
                <span className="truncate">{row.source.value || <span className="muted">(empty)</span>}</span>
                <span className={usable ? "diff-add" : "muted"}>
                  {row.status === "change" ? "Ready" : row.status === "same" ? "Unchanged" : row.status === "type" ? `Type is ${row.target?.ty}` : "Missing"}
                </span>
              </div>
            );
          })}
        </div>
        <footer className="modal-foot">
          <span className="muted small">One undo step · destination row {targetRow}</span><span className="spacer" />
          <button className="btn" onClick={onClose} disabled={busy}>Cancel</button>
          <button className="btn primary" onClick={apply} disabled={busy || !available.some((row) => selected.has(row.source.path))}>
            <Check size={14} /> Paste {available.filter((row) => selected.has(row.source.path)).length} field{available.filter((row) => selected.has(row.source.path)).length === 1 ? "" : "s"}
          </button>
        </footer>
      </div>
    </div>
  );
}
