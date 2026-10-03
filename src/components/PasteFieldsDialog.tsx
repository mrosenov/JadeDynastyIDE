import { useEffect, useMemo, useRef, useState } from "react";
import { ClipboardPaste, TriangleAlert, X } from "lucide-react";
import type { EditField } from "../schema/model";
import { fillTo } from "../schema/model";
import { describe, listFields, parseFieldList } from "../schema/fieldList";
import { hex } from "../elements/format";

interface Props {
  list: number;
  listName: string;
  itemSize: number;
  onApply: (fields: EditField[], struct?: string) => void;
  onCancel: () => void;
}

const EXAMPLE = "ID;Name;Type;Count;Value_1;Value_2;Value_3\nint32;wstring:64;int32;int32;float;float;float";

/**
 * Turns a pasted field list (a line of names and a line of types, as in
 * sELedit or Jade Editor configs) into the list's fields, with a preview of
 * where each lands in the record.
 */
export function PasteFieldsDialog({ list, listName, itemSize, onApply, onCancel }: Props) {
  const [text, setText] = useState("");
  const [fill, setFill] = useState(true);
  const area = useRef<HTMLTextAreaElement>(null);

  useEffect(() => area.current?.focus(), []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onCancel();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onCancel]);

  const parsed = useMemo(() => parseFieldList(text, itemSize), [text, itemSize]);
  const bad = parsed.fields.filter((f) => f.error).length;
  const state = parsed.size === itemSize ? "ok" : parsed.size < itemSize ? "short" : "over";
  const canApply = parsed.fields.length > 0 && bad === 0 && parsed.errors.length === 0 && state !== "over";

  const apply = () => {
    if (!canApply) return;
    const fields = listFields(parsed);
    onApply(fill && state === "short" ? fillTo(fields, itemSize) : fields, parsed.struct);
  };

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
      <div className="modal paste-dialog" role="dialog" aria-label="Paste a field list">
        <header className="modal-head">
          <ClipboardPaste size={16} className="accent-icon" />
          <h3 className="truncate">
            Paste a field list · list {list} {listName}
          </h3>
          <span className="spacer" />
          <button className="icon-btn" onClick={onCancel} aria-label="Close">
            <X size={18} />
          </button>
        </header>

        <div className="paste-body">
          <p className="muted small paste-help">
            A line of names and a line of types, separated by <code>;</code> (tabs or commas work too), as in sELedit and Jade Editor configs. A whole config block works as well. Types:{" "}
            <code>int32</code> <code>uint32</code> <code>int16</code> <code>int8</code> <code>int64</code> <code>float</code> <code>double</code>, <code>wstring:N</code> and{" "}
            <code>string:N</code> (N in bytes: <code>wstring:64</code> is 32 characters), <code>byte:N</code>, <code>byte:AUTO</code> (the rest of the record), and arrays like{" "}
            <code>int32[4]</code>.
          </p>
          <textarea
            ref={area}
            className="paste-text mono"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                e.preventDefault();
                apply();
              }
            }}
            placeholder={EXAMPLE}
            spellCheck={false}
            rows={4}
          />

          {text.trim() && (
            <>
              <div className={`se-meter ${state}`}>
                <div className="se-meter-bar">
                  <span style={{ width: `${Math.min(100, (parsed.size / Math.max(1, itemSize)) * 100)}%` }} />
                </div>
                <span className="mono">
                  {parsed.size} / {itemSize} B
                </span>
                <span className="se-meter-note">
                  {state === "ok" ? "covers the record" : state === "short" ? `${itemSize - parsed.size} B left` : `${parsed.size - itemSize} B more than the record`}
                </span>
                {parsed.struct && <span className="tag">{parsed.struct}</span>}
              </div>

              {[...parsed.errors, ...parsed.notes].map((m) => (
                <div key={m} className={"paste-note" + (parsed.errors.includes(m) ? " error" : "")}>
                  <TriangleAlert size={13} /> {m}
                </div>
              ))}
              {state === "over" && (
                <div className="paste-note error">
                  <TriangleAlert size={13} /> The fields take {parsed.size} B, but records of this list are {itemSize} B.
                </div>
              )}

              {parsed.fields.length > 0 && (
                <div className="paste-preview scroll">
                  <table>
                    <thead>
                      <tr>
                        <th>Offset</th>
                        <th>Name</th>
                        <th>Written</th>
                        <th>Read as</th>
                        <th className="num">Size</th>
                      </tr>
                    </thead>
                    <tbody>
                      {parsed.fields.map((f, i) => (
                        <tr key={i} className={f.error ? "error" : f.off + f.size > itemSize ? "over" : ""}>
                          <td className="mono muted">{f.error ? "" : hex(f.off, 4)}</td>
                          <td className="truncate">{f.name}</td>
                          <td className="mono">{f.type}</td>
                          <td>{f.error ? <span className="paste-error">{f.error}</span> : describe(f.field!)}</td>
                          <td className="num mono">{f.error ? "" : `${f.size} B`}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}

              {state === "short" && parsed.fields.length > 0 && (
                <label className="paste-fill">
                  <input type="checkbox" checked={fill} onChange={(e) => setFill(e.target.checked)} />
                  Cover the remaining {itemSize - parsed.size} B with <span className="mono">unknown_XXXX</span> fields
                </label>
              )}
            </>
          )}
        </div>

        <footer className="modal-foot">
          <span className="muted small">Replaces the draft's fields; nothing is saved until you press Save.</span>
          <span className="spacer" />
          <button className="btn" onClick={onCancel}>
            Cancel
          </button>
          <button className="btn primary" onClick={apply} disabled={!canApply} title="Ctrl+Enter">
            <ClipboardPaste size={14} /> Use {parsed.fields.length || ""} field{parsed.fields.length === 1 ? "" : "s"}
          </button>
        </footer>
      </div>
    </div>
  );
}
