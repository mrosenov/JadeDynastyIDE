import type { FieldNode } from "../elements/types";
import { type Path, flatten } from "../elements/fieldPaths";

interface Props {
  nodes: FieldNode[];
  expanded: Set<Path>;
  selected: Path | null;
  onToggle: (path: Path) => void;
  onSelect: (path: Path) => void;
  onHover: (path: Path | null) => void;
}

export function FieldTree({ nodes, expanded, selected, onToggle, onSelect, onHover }: Props) {
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
              "field-row field-grid" + (path === selected ? " active" : "") + (node.unknown ? " unknown" : "")
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
                  ▸
                </button>
              ) : (
                <span className="caret-space" />
              )}
              <span className="field-label truncate">{node.name}</span>
              {node.comment && <span className="field-comment truncate">{node.comment}</span>}
            </span>
            <span className="field-value truncate mono">
              {node.value ?? (node.children ? <span className="muted">{node.children.length} items</span> : null)}
              {node.hint && <span className="hint">{node.hint}</span>}
            </span>
            <span className="muted mono truncate">{node.ty}</span>
            <span className="muted mono">{node.off.toString(16).toUpperCase().padStart(4, "0")}</span>
          </div>
        );
      })}
    </div>
  );
}
