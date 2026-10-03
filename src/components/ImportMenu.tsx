import { useEffect, useRef, useState } from "react";
import { ClipboardPaste, Import } from "lucide-react";
import { importCandidates } from "../elements/api";
import type { ImportCandidate, ListDef } from "../elements/types";

interface Props {
  list: number;
  onImport: (def: ListDef, from: string) => void;
  /** Opens the dialog for a pasted field list. */
  onPaste: () => void;
}

/** "Import from…": this list's definition in the other versions' layouts, or a pasted field list. */
export function ImportMenu({ list, onImport, onPaste }: Props) {
  const [open, setOpen] = useState(false);
  const [candidates, setCandidates] = useState<ImportCandidate[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const anchor = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    setCandidates(null);
    setError(null);
    importCandidates(list)
      .then(setCandidates)
      .catch((e) => setError(String(e)));
  }, [open, list]);

  // Close on a click outside or Esc.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!anchor.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      }
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  return (
    <div className="import-menu-anchor" ref={anchor}>
      <button
        className={"btn small" + (open ? " active" : "")}
        onClick={() => setOpen((o) => !o)}
        title={`Copy list ${list}'s structure from another version, or from a pasted field list`}
      >
        <Import size={14} /> Import from…
      </button>
      {open && (
        <div className="import-menu" role="menu">
          <div className="import-menu-title">List {list} in other versions</div>
          {error && <div className="empty-note">{error}</div>}
          {!candidates && !error && <div className="empty-note">Loading…</div>}
          {candidates?.length === 0 && <div className="empty-note">No other version defines list {list}.</div>}
          {candidates?.map((c) => {
            const same = c.size === c.itemSize;
            return (
              <button
                key={c.layoutId}
                className="import-option"
                role="menuitem"
                onClick={() => {
                  setOpen(false);
                  onImport(c.def, c.layoutId);
                }}
                title={`Replace the draft with ${c.layoutId}'s definition (not saved until you press Save)`}
              >
                <span className="tag">{c.layoutId}</span>
                <span className="import-option-name">
                  <span className="truncate">{c.name}</span>
                  {c.structName && <span className="mono truncate">{c.structName}</span>}
                </span>
                <span
                  className={"size-badge " + (same ? "same" : "differs")}
                  title={same ? `Describes the full ${c.itemSize} B record` : `Describes ${c.size} B; records here are ${c.itemSize} B`}
                >
                  {same ? "same size" : `${c.size} B ≠ ${c.itemSize} B`}
                </span>
              </button>
            );
          })}
          <div className="import-menu-title">From text</div>
          <button
            className="import-option"
            role="menuitem"
            onClick={() => {
              setOpen(false);
              onPaste();
            }}
            title="A line of names and a line of types, as in sELedit and Jade Editor configs"
          >
            <ClipboardPaste size={15} className="accent-icon" />
            <span className="import-option-name">
              <span className="truncate">Paste a field list…</span>
              <span className="mono truncate">ID;Name;… / int32;wstring:64;…</span>
            </span>
          </button>
        </div>
      )}
    </div>
  );
}
