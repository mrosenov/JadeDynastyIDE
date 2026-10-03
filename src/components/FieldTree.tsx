import type { FieldNode } from "../elements/types";
import { TIME_ROLES, formatDaytime, formatDuration, formatUnix, formatUnixUtc } from "../elements/time";
import { type Path, flatten } from "../elements/fieldPaths";
import { DIALOGS } from "../tabs";
import { isTextNode, textLines } from "../elements/text";
import { CalendarClock, ChevronRight, Clock, CornerDownLeft, GitBranch, Timer } from "lucide-react";
import { decodeValue, isChanged, isEditable } from "../elements/edit";
import { InlineEditor } from "./InlineEditor";
import { coins, formatMoneyWords } from "../elements/money";

/** A copper amount as gold, silver and copper coins. */
function MoneyChip({ value }: { value: string }) {
  const n = Number(value);
  if (!Number.isFinite(n)) return null;
  const c = coins(n);
  const parts: [number, string, string][] = [
    [c.gold, "G", "gold"],
    [c.silver, "S", "silver"],
    [c.copper, "C", "copper"],
  ];
  const shown = parts.filter(([v]) => v > 0);
  return (
    <span className="money-chip" title={`${formatMoneyWords(n)} · ${n.toLocaleString()} Copper\n100 Copper = 1 Silver, 100 Silver = 1 Gold`}>
      {c.negative && "−"}
      {(shown.length ? shown : [parts[2]]).map(([v, unit, kind]) => (
        <span key={kind} className={`coin ${kind}`}>
          {v.toLocaleString()}
          <i>{unit}</i>
        </span>
      ))}
    </span>
  );
}

/** A time value read for people: a date, a duration or a time of day. */
function TimeChip({ role, value }: { role: string; value: string }) {
  const n = Number(value);
  if (role === "time") {
    const local = formatUnix(n);
    return local ? (
      <span className="time-chip" title={`${formatUnixUtc(n)} · unix ${value}`}>
        <CalendarClock size={11} /> {local}
      </span>
    ) : (
      <span className="time-chip unset">not set</span>
    );
  }
  if (role === "daytime") {
    const clock = formatDaytime(n);
    return clock ? (
      <span className="time-chip daytime" title={`Time of day: ${value} seconds after midnight`}>
        <Clock size={11} /> {clock}
      </span>
    ) : (
      <span className="time-chip unset" title="Not between 0 and 86400 seconds">
        not a time of day
      </span>
    );
  }
  const ms = role === "duration_ms";
  if (n === 0) return <span className="time-chip unset">none</span>;
  return (
    <span className="time-chip duration" title={`Duration: ${value} ${ms ? "milliseconds" : "seconds"}`}>
      <Timer size={11} /> {formatDuration(ms ? n / 1000 : n)}
    </span>
  );
}

/** Where a value leads: a record, or an NPC dialog. */
const link = (node: FieldNode): [number, number] | undefined =>
  node.link ?? (node.talk !== undefined ? [DIALOGS, node.talk] : undefined);

/** A text on one line, its line breaks shown as ↵ marks. */
function OneLine({ text }: { text: string }) {
  const lines = textLines(text);
  return (
    <span className="text-value truncate">
      {lines.map((line, i) => (
        <span key={i}>
          {i > 0 && <CornerDownLeft size={11} className="nl-mark" aria-label="line break" />}
          {line}
        </span>
      ))}
    </span>
  );
}

interface Props {
  nodes: FieldNode[];
  expanded: Set<Path>;
  selected: Path | null;
  onToggle: (path: Path) => void;
  onSelect: (path: Path) => void;
  onHover: (path: Path | null) => void;
  /** Follow a reference; `newTab` for Ctrl+click or middle-click. */
  onFollow: (list: number, row: number, newTab?: boolean) => void;
  /** Item icon URL for a path ID (when the client's icons are available). */
  icon?: (pathId?: number | null) => string | undefined;
  /** A value named by an enum or mask was clicked (shows all its values). */
  onSet?: (node: FieldNode, anchor: DOMRect) => void;
  /** The field being edited in place. */
  editing?: Path | null;
  /** Double-click, Enter or F2 on an editable field (texts open their editor). */
  onStartEdit?: (path: Path, node: FieldNode) => void;
  /** Saves a value; resolves to an error message, or null when applied. */
  onCommit?: (node: FieldNode, value: string) => Promise<string | null>;
  onCancelEdit?: () => void;
  /** The record's bytes now and as the file was opened (marks changed fields). */
  bytes?: number[];
  original?: number[];
}

export function FieldTree({ nodes, expanded, selected, onToggle, onSelect, onHover, onFollow, icon, onSet, editing, onStartEdit, onCommit, onCancelEdit, bytes, original }: Props) {
  const rows = flatten(nodes, expanded);
  const onKeyDown = (e: React.KeyboardEvent) => {
    if ((e.key === "Enter" || e.key === "F2") && selected && !editing && onStartEdit) {
      const row = rows.find((r) => r.path === selected);
      if (row && isEditable(row.node)) {
        e.preventDefault();
        onStartEdit(row.path, row.node);
      }
    }
  };
  return (
    <div className="fields scroll" onMouseLeave={() => onHover(null)} tabIndex={0} onKeyDown={onKeyDown}>
      <div className="table-head field-grid">
        <span>Field</span>
        <span>Value</span>
        <span>Type</span>
        <span>Offset</span>
      </div>
      {rows.map(({ node, path, depth }) => {
        const open = expanded.has(path);
        const editable = !!onStartEdit && isEditable(node);
        const changed = !!bytes && isChanged(node, bytes, original);
        const was = changed && !node.children ? decodeValue(node.ty, original!, node.off) : null;
        return (
          <div
            key={path}
            className={
              "field-row field-grid" +
              (path === selected ? " active" : "") +
              (node.unknown ? " unknown" : "") +
              (node.group ? " group" : "") +
              (changed ? " changed" : "") +
              (editable ? " editable" : "")
            }
            onClick={() => onSelect(path)}
            onDoubleClick={() => (node.children ? onToggle(path) : editable && onStartEdit!(path, node))}
            onMouseEnter={() => onHover(path)}
            title={changed ? `Changed${was !== null ? ` · was: ${was === "" ? "(empty)" : was}` : ""}${node.comment ? `
${node.comment}` : ""}` : node.comment}
          >
            <span className="field-name" style={{ paddingLeft: 8 + depth * 16 }}>
              {node.children ? (
                <button
                  className={"caret" + (open ? " open" : "")}
                  onClick={(e) => {
                    e.stopPropagation();
                    onToggle(path);
                  }}
                  aria-label={open ? "Collapse" : "Expand"}
                >
                  <ChevronRight size={15} />
                </button>
              ) : (
                <span className="caret-space" />
              )}
              <span className="field-label truncate">{node.name}</span>
              {node.comment && <span className="field-comment truncate">{node.comment}</span>}
            </span>
            {editing === path && onCommit && onCancelEdit ? (
              <span className="field-value editing">
                <InlineEditor node={node} onCommit={(v) => onCommit(node, v)} onCancel={onCancelEdit} />
              </span>
            ) : (
            <span className="field-value truncate mono">
              {node.icon && icon?.(node.icon) && <img className="field-icon" src={icon(node.icon)} alt="" draggable={false} />}
              {isTextNode(node) ? (
                <OneLine text={node.value!} />
              ) : (
                (node.value ?? (node.children ? <span className="muted">{node.children.length} items</span> : null))
              )}
              {node.hint &&
                (link(node) ? (
                  <button
                    className="hint link-hint"
                    title="Open the referenced record (Ctrl+click or middle-click: in a new tab)"
                    onClick={(e) => {
                      e.stopPropagation();
                      onFollow(link(node)![0], link(node)![1], e.ctrlKey || e.metaKey);
                    }}
                    onAuxClick={(e) => {
                      if (e.button !== 1) return;
                      e.preventDefault();
                      e.stopPropagation();
                      onFollow(link(node)![0], link(node)![1], true);
                    }}
                    onMouseDown={(e) => e.button === 1 && e.preventDefault()}
                  >
                    {node.hint}
                  </button>
                ) : node.set && onSet ? (
                  <button
                    className="hint set-hint"
                    title="Show every value of this enum or mask"
                    onClick={(e) => {
                      e.stopPropagation();
                      onSet(node, e.currentTarget.getBoundingClientRect());
                    }}
                  >
                    {node.hint}
                  </button>
                ) : (
                  <span className="hint">{node.hint}</span>
                ))}
              {node.display && TIME_ROLES.has(node.display) && node.value !== undefined && <TimeChip role={node.display} value={node.value} />}
              {node.display === "money" && node.value !== undefined && <MoneyChip value={node.value} />}
              {node.display && !TIME_ROLES.has(node.display) && node.display !== "money" && <span className="role">{node.display}</span>}
              {node.cond && (
                <span className="cond-chip" title={`Conditional type: ${node.cond}`}>
                  <GitBranch size={11} /> {node.cond}
                </span>
              )}
            </span>
            )}
            <span
              className="muted mono truncate"
              title={
                node.ty.startsWith("wchar[")
                  ? `${node.ty} · ${node.size / 2} characters = ${node.size} bytes`
                  : `${node.ty} · ${node.size} byte${node.size === 1 ? "" : "s"}`
              }
            >
              {node.ty}
            </span>
            <span className="muted mono">{node.off.toString(16).toUpperCase().padStart(4, "0")}</span>
          </div>
        );
      })}
    </div>
  );
}
