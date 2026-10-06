import { useEffect, useState } from "react";
import { Check, CircleAlert, Loader2 } from "lucide-react";
import type { FieldEdit, FieldNode } from "../elements/types";
import { FLOAT_TYPES, INTEGER_TYPES } from "../elements/edit";

type Operation = "set" | "add" | "subtract" | "multiply" | "divide";

export interface QuickField {
  path: string;
  node: FieldNode;
}

interface Props {
  fields: QuickField[];
  onApply: (edits: FieldEdit[], label: string) => Promise<string | null>;
  onClear: () => void;
}

const INTEGER_PRESETS = ["0", "1", "2", "5", "10", "100", "500", "1000", "5000", "10000"];
const FLOAT_PRESETS = ["0.1", "0.2", "0.25", "0.33", "0.4", "0.5", "0.66", "0.7", "0.75", "0.8"];

const OP_LABEL: Record<Operation, string> = {
  set: "=",
  add: "+",
  subtract: "−",
  multiply: "×",
  divide: "÷",
};

function integer(text: string): bigint | null {
  try {
    return BigInt(text.trim());
  } catch {
    return null;
  }
}

/** Calculate one new value without losing precision from 64-bit integers. */
function calculate(node: FieldNode, operation: Operation, operand: string): string {
  const raw = operand.trim();
  if (!raw) throw new Error("Enter a value.");

  if (INTEGER_TYPES.has(node.ty)) {
    const by = integer(raw);
    if (by === null) throw new Error(`“${raw}” is not a whole number for ${node.name}.`);
    if (operation === "set") return raw;
    const old = integer(node.value ?? "");
    if (old === null) throw new Error(`${node.name} does not contain a whole number.`);
    if (operation === "divide") {
      if (by === 0n) throw new Error("Cannot divide by zero.");
      if (old % by !== 0n) throw new Error(`${node.name}: ${old} ÷ ${by} is not a whole number.`);
      return String(old / by);
    }
    if (operation === "add") return String(old + by);
    if (operation === "subtract") return String(old - by);
    return String(old * by);
  }

  if (FLOAT_TYPES.has(node.ty)) {
    const by = Number(raw);
    if (!Number.isFinite(by)) throw new Error(`“${raw}” is not a number.`);
    if (operation === "set") return raw;
    const old = Number(node.value);
    if (!Number.isFinite(old)) throw new Error(`${node.name} does not contain a number.`);
    if (operation === "divide" && by === 0) throw new Error("Cannot divide by zero.");
    const next = operation === "add" ? old + by : operation === "subtract" ? old - by : operation === "multiply" ? old * by : old / by;
    if (!Number.isFinite(next)) throw new Error(`${node.name} would become too large.`);
    return Object.is(next, -0) ? "0" : String(next);
  }

  throw new Error(`${node.name} is not a number field.`);
}

export function QuickEditor({ fields, onApply, onClear }: Props) {
  const [operation, setOperation] = useState<Operation>("set");
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const hasInteger = fields.some(({ node }) => INTEGER_TYPES.has(node.ty));

  useEffect(() => setError(null), [fields, operation, value]);

  const apply = async () => {
    if (!fields.length || busy) return;
    let edits: FieldEdit[];
    try {
      edits = fields.map(({ node }) => ({ off: node.off, value: calculate(node, operation, value) }));
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
      return;
    }
    setBusy(true);
    const symbol = OP_LABEL[operation];
    const problem = await onApply(edits, `Quick edit ${fields.length} field${fields.length === 1 ? "" : "s"}: ${symbol} ${value.trim()}`);
    setBusy(false);
    if (problem) setError(problem.replace(/^Error: /, ""));
  };

  return (
    <section className="quick-editor" aria-label="Quick edit selected fields">
      <div className="quick-editor-head">
        <strong>Quick edit</strong>
        <span className="muted small" title={fields.length ? fields.map(({ node }) => `${node.name} = ${node.value}`).join("\n") : undefined}>
          {fields.length ? `${fields.length} number field${fields.length === 1 ? "" : "s"} selected` : "Select number fields using their checkboxes"}
        </span>
        <span className="spacer" />
        {fields.length > 0 && (
          <button className="link" onClick={onClear} disabled={busy}>
            Clear selection
          </button>
        )}
      </div>
      <div className="quick-editor-controls">
        <select value={operation} onChange={(event) => setOperation(event.target.value as Operation)} aria-label="Quick edit operation" disabled={!fields.length || busy}>
          <option value="set">= Set</option>
          <option value="add">+ Add</option>
          <option value="subtract">− Subtract</option>
          <option value="multiply">× Multiply</option>
          <option value="divide">÷ Divide</option>
        </select>
        <input
          className={"mono" + (error ? " invalid" : "")}
          value={value}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              apply();
            }
          }}
          placeholder={hasInteger ? "Whole number" : "Number"}
          aria-label="Quick edit value"
          spellCheck={false}
          disabled={!fields.length || busy}
        />
        <button className="btn primary small quick-apply" onClick={apply} disabled={!fields.length || !value.trim() || busy}>
          {busy ? <Loader2 size={13} className="spin" /> : <Check size={13} />} Apply
        </button>
        <div className="quick-presets" aria-label="Common values">
          {!hasInteger && FLOAT_PRESETS.map((preset) => <button key={preset} onClick={() => setValue(preset)} disabled={!fields.length || busy}>{preset}</button>)}
          {INTEGER_PRESETS.map((preset) => <button key={preset} onClick={() => setValue(preset)} disabled={!fields.length || busy}>{Number(preset).toLocaleString()}</button>)}
        </div>
      </div>
      {error && <div className="quick-editor-error" role="alert"><CircleAlert size={13} /> {error}</div>}
    </section>
  );
}
