import type { FieldNode } from "../elements/types";
import { formatUnix, formatUnixUtc } from "../elements/time";
import { type Path, flatten } from "../elements/fieldPaths";
import { DIALOGS } from "../tabs";
import { isTextNode, textLines } from "../elements/text";
import { CalendarClock, ChevronRight, CornerDownLeft, GitBranch } from "lucide-react";

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
}

export function FieldTree({ nodes, expanded, selected, onToggle, onSelect, onHover, onFollow, icon, onSet }: Props) {
  const rows = flatten(nodes, expanded);
  return (
    <div className="fields scroll" onMouseLeave={() => onHover(null)}>
      <div className="table-head field-grid">
        <span>Field</span>
        <span>Value</span>
        <span>Type</span>
        <span>Offset</span>
      </div>
      {rows.map(({ node, path, depth }) => {
        const open = expanded.has(path);
        return (
          <div
            key={path}
            className={
              "field-row field-grid" + (path === selected ? " active" : "") + (node.unknown ? " unknown" : "") + (node.group ? " group" : "")
            }
            onClick={() => onSelect(path)}
            onDoubleClick={() => node.children && onToggle(path)}
            onMouseEnter={() => onHover(path)}
            title={node.comment}
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
              {node.display === "time" && node.value !== undefined && (() => {
                const seconds = Number(node.value);
                const local = formatUnix(seconds);
                return local ? (
                  <span className="time-chip" title={`${formatUnixUtc(seconds)} · unix ${node.value}`}>
                    <CalendarClock size={11} /> {local}
                  </span>
                ) : (
                  <span className="time-chip unset">not set</span>
                );
              })()}
              {node.display && node.display !== "time" && <span className="role">{node.display}</span>}
              {node.cond && (
                <span className="cond-chip" title={`Conditional type: ${node.cond}`}>
                  <GitBranch size={11} /> {node.cond}
                </span>
              )}
            </span>
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
