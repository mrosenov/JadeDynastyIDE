import { Braces, ChevronLeft, ChevronRight, CircleHelp, Plus } from "lucide-react";
import { type Reading, type Span, allZero, readings } from "../elements/readings";
import { hex } from "../elements/format";

interface Props {
  bytes: number[];
  offset: number;
  span: Span;
  /** Label of the define action, e.g. "Define in Schema Editor". */
  actionLabel: string;
  onOffset: (offset: number) => void;
  onDefine: (offset: number, reading: Reading["define"]) => void;
}

/** Possible readings of undefined bytes, each one a field type to define. */
export function ReadingsCard({ bytes, offset, span, actionLabel, onOffset, onDefine }: Props) {
  const list = readings(bytes, offset, span);
  const best = list.find((r) => r.likely) ?? list[0];
  const zero = allZero(bytes, offset, Math.min(4, span.end - offset));

  return (
    <div className="readings">
      <div className="readings-head">
        <CircleHelp size={15} />
        <span>
          Unknown bytes: the schema doesn't describe this part of the record yet. Possible readings at{" "}
          <span className="mono">{hex(offset, 4)}</span>:
        </span>
        <span className="spacer" />
        <button
          className="icon-btn small"
          onClick={() => onOffset(offset - 1)}
          disabled={offset <= span.start}
          title="Start one byte earlier"
        >
          <ChevronLeft size={15} />
        </button>
        <button
          className="icon-btn small"
          onClick={() => onOffset(offset + 1)}
          disabled={offset + 1 >= span.end}
          title="Start one byte later"
        >
          <ChevronRight size={15} />
        </button>
      </div>
      {zero && <div className="readings-note">These bytes are zero, so every number type reads 0. Look at other records to tell them apart.</div>}
      <div className="readings-list">
        {list.map((r) => (
          <button
            key={r.key}
            className={"reading" + (r.likely ? " likely" : "")}
            onClick={() => onDefine(offset, r.define)}
            title={`Define a ${r.label} field at ${hex(offset, 4)} (${r.define.size} B)`}
          >
            <span className="reading-label">{r.label}</span>
            <span className="reading-value mono truncate">
              {r.value}
              {r.detail && <span className="reading-detail"> · {r.detail}</span>}
            </span>
            <span className="reading-use">
              <Plus size={13} /> use
            </span>
          </button>
        ))}
      </div>
      {best && (
        <button className="btn readings-action" onClick={() => onDefine(offset, best.define)}>
          <Braces size={15} /> {actionLabel}
          <span className="muted">as {best.label}</span>
        </button>
      )}
    </div>
  );
}
