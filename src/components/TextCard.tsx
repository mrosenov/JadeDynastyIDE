import { useState } from "react";
import { Check, Copy, Type, X } from "lucide-react";
import type { FieldNode } from "../elements/types";
import { hasColours, styledLines, textLines } from "../elements/text";

interface Props {
  node: FieldNode;
  onClose: () => void;
}

/**
 * A selected text field as the game shows it: line breaks applied and
 * ^RRGGBB colour codes rendered, with the raw text a click away.
 */
export function TextCard({ node, onClose }: Props) {
  const [raw, setRaw] = useState(false);
  const [copied, setCopied] = useState(false);
  const text = node.value ?? "";
  const lines = styledLines(text);
  const coloured = hasColours(text);

  const copy = () => {
    // Copied as stored, with CR LF breaks.
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1200);
  };

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
        <button className="icon-btn small" onClick={copy} title="Copy the text">
          {copied ? <Check size={13} /> : <Copy size={13} />}
        </button>
        <button className="icon-btn small" onClick={onClose} aria-label="Close">
          <X size={15} />
        </button>
      </div>
      <div className="text-preview">
        {raw
          ? textLines(text).map((line, i) => (
              <div key={i} className="text-line mono">
                {line || " "}
              </div>
            ))
          : lines.map((runs, i) => (
              <div key={i} className="text-line">
                {runs.length
                  ? runs.map((r, j) => (
                      <span key={j} style={r.colour ? { color: r.colour } : undefined}>
                        {r.text}
                      </span>
                    ))
                  : " "}
              </div>
            ))}
      </div>
    </div>
  );
}
