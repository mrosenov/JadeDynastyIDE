import { useEffect, useState } from "react";
import type { ListSummary, RecordDetail, RecordRow } from "../elements/types";
import { LAYOUT_HELP, LAYOUT_LABEL, hex } from "../elements/format";
import { FieldTree } from "./FieldTree";
import { type Path, allPaths, nodeAt, pathAt } from "../elements/fieldPaths";
import { HexView } from "./HexView";

interface Props {
  list: ListSummary;
  row: RecordRow | null;
  detail: RecordDetail | null;
}

export function RecordInspector({ list, row, detail }: Props) {
  const [expanded, setExpanded] = useState<Set<Path>>(new Set());
  const [selected, setSelected] = useState<Path | null>(null);
  const [hovered, setHovered] = useState<Path | null>(null);

  // Keep the expansion state while browsing records of the same list.
  useEffect(() => setExpanded(new Set()), [list.index]);
  useEffect(() => setHovered(null), [detail]);

  if (!row || !detail) {
    return (
      <section className="pane inspector">
        <div className="empty-note center">Select a record to inspect it.</div>
      </section>
    );
  }

  const toggle = (path: Path) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });

  const pickOffset = (offset: number) => {
    const chain = pathAt(detail.nodes, offset);
    if (!chain.length) return;
    setExpanded((prev) => new Set([...prev, ...chain.slice(0, -1)]));
    setSelected(chain[chain.length - 1]);
  };

  const active = nodeAt(detail.nodes, hovered ?? selected ?? "") ?? null;
  const focused = selected ? nodeAt(detail.nodes, selected) : null;
  const unknownBytes = detail.nodes.filter((n) => n.unknown).reduce((sum, n) => sum + n.size, 0);

  return (
    <section className="pane inspector">
      <div className="inspector-head">
        <div className="inspector-title">
          <h2 className="truncate">{row.name || <span className="muted">Unnamed record</span>}</h2>
          <span className={`badge fit-${detail.layout}`} title={LAYOUT_HELP[detail.layout]}>
            {LAYOUT_LABEL[detail.layout]}
          </span>
        </div>
        <dl className="facts">
          <div>
            <dt>ID</dt>
            <dd className="mono">{row.id}</dd>
          </div>
          <div>
            <dt>Index</dt>
            <dd className="mono">{row.index}</dd>
          </div>
          <div>
            <dt>File offset</dt>
            <dd className="mono">{hex(detail.fileOffset, 8)}</dd>
          </div>
          <div>
            <dt>Size</dt>
            <dd className="mono">
              {detail.bytes.length} B
              {detail.layoutSize !== null && detail.layoutSize !== detail.bytes.length && (
                <span className="muted"> (layout {detail.layoutSize} B)</span>
              )}
            </dd>
          </div>
          {unknownBytes > 0 && detail.layout !== "none" && (
            <div>
              <dt>Unknown</dt>
              <dd className="mono">{unknownBytes} B</dd>
            </div>
          )}
        </dl>
        {(detail.layout === "partial" || detail.layout === "approx") && (
          <p className="note">{LAYOUT_HELP[detail.layout]}</p>
        )}
      </div>

      <div className="inspector-body">
        <div className="subhead">
          <span className="pane-title">Fields</span>
          <span className="spacer" />
          <button className="link" onClick={() => setExpanded(new Set(allPaths(detail.nodes)))}>
            Expand all
          </button>
          <button className="link" onClick={() => setExpanded(new Set())}>
            Collapse all
          </button>
        </div>
        <FieldTree
          nodes={detail.nodes}
          expanded={expanded}
          selected={selected}
          onToggle={toggle}
          onSelect={setSelected}
          onHover={setHovered}
        />
        <div className="subhead">
          <span className="pane-title">Bytes</span>
          <span className="spacer" />
          {active && (
            <span className="muted mono">
              {active.name} · {hex(active.off)}–{hex(active.off + active.size - 1)} · {active.size} B
            </span>
          )}
        </div>
        <HexView
          bytes={detail.bytes}
          fileOffset={detail.fileOffset}
          highlight={active ? { off: active.off, size: active.size } : null}
          focusKey={focused ? `${detail.index}:${selected}` : null}
          onPick={pickOffset}
        />
      </div>
    </section>
  );
}
