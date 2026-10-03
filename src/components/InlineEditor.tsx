import { useEffect, useRef, useState } from "react";
import { Check, CircleAlert, Loader2, X } from "lucide-react";
import { namedSet } from "../elements/api";
import type { FieldNode, SetDetail } from "../elements/types";

interface Props {
  node: FieldNode;
  /** Saves the value; resolves to an error message, or null when it was applied. */
  onCommit: (value: string) => Promise<string | null>;
  onCancel: () => void;
}

const OTHER = "__other__";

/**
 * Edits a value in place: a dropdown for enums (with "other value" for ones
 * the enum lacks), true/false for bools, else a text box. Enter or a choice
 * saves, Esc cancels; a value the field cannot hold keeps the editor open
 * with the reason.
 */
export function InlineEditor({ node, onCommit, onCancel }: Props) {
  const [value, setValue] = useState(node.value ?? "");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [set, setSet] = useState<SetDetail | null>(null);
  const [free, setFree] = useState(false);
  const input = useRef<HTMLInputElement>(null);

  // Enums offer their values; masks are edited as numbers (the calculator applies bits).
  useEffect(() => {
    if (!node.set) return;
    namedSet(node.set)
      .then((d) => setSet(d.kind === "enum" ? d : null))
      .catch(() => setSet(null));
  }, [node.set]);

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, [free, set]);

  const commit = async (v: string) => {
    if (busy) return;
    if (v === node.value) return onCancel();
    setBusy(true);
    setError(null);
    const problem = await onCommit(v);
    setBusy(false);
    if (problem) setError(problem.replace(/^Error: /, ""));
  };

  const onKey = (e: React.KeyboardEvent) => {
    e.stopPropagation();
    if (e.key === "Enter") {
      e.preventDefault();
      commit(value);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  };

  const values = set?.set.values ?? [];
  const inEnum = values.some((v) => String(v.value) === value);
  const isBool = node.ty === "bool";

  return (
    <span className="inline-editor" onClick={(e) => e.stopPropagation()} onDoubleClick={(e) => e.stopPropagation()}>
      {set && !free ? (
        <select
          className="ie-control"
          autoFocus
          value={inEnum ? value : OTHER}
          onChange={(e) => {
            if (e.target.value === OTHER) {
              setFree(true);
              return;
            }
            setValue(e.target.value);
            commit(e.target.value);
          }}
          onKeyDown={onKey}
          onBlur={() => !busy && onCancel()}
        >
          {values.map((v) => (
            <option key={v.value} value={String(v.value)}>
              {v.value} · {v.label}
            </option>
          ))}
          <option value={OTHER}>{inEnum ? "Other value…" : `${value} (not in the enum) · other value…`}</option>
        </select>
      ) : isBool ? (
        <select className="ie-control" autoFocus value={value} onChange={(e) => commit(e.target.value)} onKeyDown={onKey} onBlur={() => !busy && onCancel()}>
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
      ) : (
        <>
          <input
            ref={input}
            className={"ie-control mono" + (error ? " invalid" : "")}
            value={value}
            onChange={(e) => {
              setValue(e.target.value);
              setError(null);
            }}
            onKeyDown={onKey}
            spellCheck={false}
            aria-label={`New value of ${node.name}`}
          />
          <button className="icon-btn small ie-ok" onMouseDown={(e) => e.preventDefault()} onClick={() => commit(value)} title="Save (Enter)" disabled={busy}>
            {busy ? <Loader2 size={13} className="spin" /> : <Check size={14} />}
          </button>
          <button className="icon-btn small" onMouseDown={(e) => e.preventDefault()} onClick={onCancel} title="Cancel (Esc)">
            <X size={14} />
          </button>
        </>
      )}
      {error && (
        <span className="ie-error" role="alert">
          <CircleAlert size={12} /> {error}
        </span>
      )}
    </span>
  );
}
