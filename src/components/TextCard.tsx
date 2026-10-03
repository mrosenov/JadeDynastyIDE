import { useEffect, useRef, useState } from "react";
import { Check, CircleAlert, Copy, Loader2, Palette, Pencil, Type, X } from "lucide-react";
import type { FieldNode } from "../elements/types";
import { hasColours, textLines } from "../elements/text";
import { textCapacity, textLength } from "../elements/edit";
import { GameText } from "./GameText";

interface Props {
  node: FieldNode;
  onClose: () => void;
  /** Saves new text; resolves to an error message, or null when applied. */
  onSave?: (value: string) => Promise<string | null>;
  /** Open in edit mode (double-click, Enter or F2 on the field). */
  editing?: boolean;
  onEditingChange?: (editing: boolean) => void;
}

/**
 * A selected text field as the game shows it: line breaks applied and
 * ^RRGGBB colour codes rendered, with the raw text a click away. In edit mode
 * the text is edited with a live preview.
 */
export function TextCard({ node, onClose, onSave, editing = false, onEditingChange }: Props) {
  const [raw, setRaw] = useState(false);
  const [copied, setCopied] = useState(false);
  const text = node.value ?? "";
  const coloured = hasColours(text);

  const copy = () => {
    // Copied as stored, with CR LF breaks.
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1200);
  };

  if (editing && onSave) {
    return <TextEditor node={node} onSave={onSave} onDone={() => onEditingChange?.(false)} />;
  }

  return (
    <div className="readings text-card">
      <div className="readings-head">
        <Type size={15} className="text-card-icon" />
        <span className="truncate">
          <b>{node.name}</b>{" "}
          <span className="muted">
            · {textLines(text).length} line{textLines(text).length === 1 ? "" : "s"} · {[...text].length} characters
          </span>
        </span>
        <span className="spacer" />
        {coloured && (
          <button className="link" onClick={() => setRaw((r) => !r)} title="Toggle between the rendered text and its colour codes">
            {raw ? "Rendered" : "Codes"}
          </button>
        )}
        {onSave && (
          <button className="icon-btn small" onClick={() => onEditingChange?.(true)} title="Edit the text (Enter or F2 on the field)">
            <Pencil size={13} />
          </button>
        )}
        <button className="icon-btn small" onClick={copy} title="Copy the text">
          {copied ? <Check size={13} /> : <Copy size={13} />}
        </button>
        <button className="icon-btn small" onClick={onClose} aria-label="Close">
          <X size={15} />
        </button>
      </div>
      <GameText text={text} raw={raw} className="text-preview" />
    </div>
  );
}

const COLOURS = ["ffffff", "ffcb4a", "ff0000", "00ff00", "00b4ff", "ff6000", "c297ff", "8b8b8b"];

/** Edits a text with a live game preview, a length counter and colour codes. */
function TextEditor({ node, onSave, onDone }: { node: FieldNode; onSave: (value: string) => Promise<string | null>; onDone: () => void }) {
  // Edited with plain line breaks; saved as CR LF.
  const [draft, setDraft] = useState((node.value ?? "").replace(/\r\n/g, "\n"));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [colour, setColour] = useState("#ffcb4a");
  const area = useRef<HTMLTextAreaElement>(null);
  const capacity = textCapacity(node.ty);
  const length = capacity ? textLength(draft, capacity.unit) : draft.length;
  const tooLong = !!capacity && length > capacity.max;

  useEffect(() => {
    area.current?.focus();
  }, []);

  const insert = (code: string) => {
    const el = area.current;
    if (!el) return;
    const { selectionStart: a, selectionEnd: b } = el;
    const next = draft.slice(0, a) + code + draft.slice(b);
    setDraft(next);
    requestAnimationFrame(() => {
      el.focus();
      el.setSelectionRange(a + code.length, a + code.length);
    });
  };

  const save = async () => {
    if (busy || tooLong) return;
    if (draft === (node.value ?? "").replace(/\r\n/g, "\n")) return onDone();
    setBusy(true);
    setError(null);
    const problem = await onSave(draft);
    setBusy(false);
    if (problem) setError(problem.replace(/^Error: /, ""));
    else onDone();
  };

  return (
    <div
      className="readings text-card text-editor"
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Escape") onDone();
        else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
          e.preventDefault();
          save();
        }
      }}
    >
      <div className="readings-head">
        <Pencil size={15} className="text-card-icon" />
        <span className="truncate">
          Editing <b>{node.name}</b>
        </span>
        <span className="spacer" />
        {capacity && (
          <span className={"text-count mono" + (tooLong ? " over" : "")} title={`This field holds ${capacity.max} ${capacity.unit} (one is kept for the terminator)`}>
            {length} / {capacity.max} {capacity.unit === "bytes" ? "B" : ""}
          </span>
        )}
      </div>
      <textarea
        ref={area}
        className={"text-area" + (tooLong ? " invalid" : "")}
        value={draft}
        onChange={(e) => {
          setDraft(e.target.value);
          setError(null);
        }}
        spellCheck={false}
        rows={Math.min(10, Math.max(3, draft.split("\n").length + 1))}
      />
      <div className="text-tools">
        <span className="muted small">
          <Palette size={12} /> Colour
        </span>
        {COLOURS.map((c) => (
          <button key={c} className="swatch" style={{ background: `#${c}` }} onClick={() => insert(`^${c}`)} title={`Insert ^${c}`} />
        ))}
        <input type="color" value={colour} onChange={(e) => setColour(e.target.value)} title="Pick a colour" />
        <button className="link small" onClick={() => insert(`^${colour.slice(1)}`)}>
          Insert ^{colour.slice(1)}
        </button>
      </div>
      <div className="muted small">Preview</div>
      <GameText text={draft || " "} className="text-preview" />
      {(error || tooLong) && (
        <div className="se-problems">
          <CircleAlert size={13} /> {error ?? `Too long: this field holds ${capacity!.max} ${capacity!.unit}.`}
        </div>
      )}
      <div className="text-actions">
        <span className="muted small">Ctrl+Enter saves · Esc cancels</span>
        <span className="spacer" />
        <button className="btn small" onClick={onDone}>
          Cancel
        </button>
        <button className="btn primary small" onClick={save} disabled={busy || tooLong}>
          {busy ? <Loader2 size={13} className="spin" /> : <Check size={13} />} Save text
        </button>
      </div>
    </div>
  );
}
