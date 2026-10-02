import type { FieldNode } from "../elements/types";
import { type Path, flatten } from "../elements/fieldPaths";
import { ChevronRight, GitBranch } from "lucide-react";

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
              {node.value ?? (node.children ? <span className="muted">{node.children.length} items</span> : null)}
              {node.hint &&
                (node.link ? (
                  <button
                    className="hint link-hint"
                    title="Open the referenced record (Ctrl+click or middle-click: in a new tab)"
                    onClick={(e) => {
                      e.stopPropagation();
                      onFollow(node.link![0], node.link![1], e.ctrlKey || e.metaKey);
                    }}
                    onAuxClick={(e) => {
                      if (e.button !== 1) return;
                      e.preventDefault();
                      e.stopPropagation();
                      onFollow(node.link![0], node.link![1], true);
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
              {node.display && <span className="role">{node.display}</span>}
              {node.cond && (
                <span className="cond-chip" title={`Conditional type: ${node.cond}`}>
                  <GitBranch size={11} /> {node.cond}
                </span>
              )}
            </span>
            <span className="muted mono truncate">{node.ty}</span>
            <span className="muted mono">{node.off.toString(16).toUpperCase().padStart(4, "0")}</span>
          </div>
        );
      })}
    </div>
  );
}
