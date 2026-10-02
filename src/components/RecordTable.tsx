import { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { ListSummary, RecordRow } from "../elements/types";
import { count } from "../elements/format";

interface Props {
  list: ListSummary;
  rows: RecordRow[] | null;
  selected: number | null;
  onSelect: (index: number) => void;
  /** Double-click: open the record in a tab that stays open. */
  onOpen?: (index: number) => void;
  /** Item icon URL for a path ID (when the client's icons are available). */
  icon?: (pathId?: number | null) => string | undefined;
  /** Replaces "count × size" in the header (e.g. for NPC dialogs). */
  meta?: string;
}

const ROW_HEIGHT = 28;

export function RecordTable({ list, rows, selected, onSelect, onOpen, icon, meta }: Props) {
  const [filter, setFilter] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => setFilter(""), [list.index]);

  const visible = useMemo(() => {
    if (!rows) return [];
    const q = filter.trim().toLowerCase();
    if (!q) return rows;
    return rows.filter((r) => String(r.id).startsWith(q) || r.name.toLowerCase().includes(q));
  }, [rows, filter]);

  const virtualizer = useVirtualizer({
    count: visible.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  const position = visible.findIndex((r) => r.index === selected);
  const withIcons = !!icon && !!rows?.some((r) => r.icon);
  const grid = "record-grid" + (withIcons ? " with-icons" : "");

  useEffect(() => {
    if (position >= 0) virtualizer.scrollToIndex(position, { align: "auto" });
  }, [position, virtualizer]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!visible.length) return;
    const step = { ArrowDown: 1, ArrowUp: -1, PageDown: 20, PageUp: -20 }[e.key];
    if (step !== undefined) {
      e.preventDefault();
      const next = Math.min(visible.length - 1, Math.max(0, (position < 0 ? -1 : position) + step));
      onSelect(visible[next].index);
    } else if (e.key === "Enter" && position >= 0) {
      e.preventDefault();
      onOpen?.(visible[position].index);
    } else if (e.key === "Home" || e.key === "End") {
      e.preventDefault();
      onSelect(visible[e.key === "Home" ? 0 : visible.length - 1].index);
    }
  };

  return (
    <section className="pane records">
      <div className="pane-head">
        <span className="pane-title" title={list.structName ?? undefined}>
          {list.name}
        </span>
        <span className="muted">
          {rows && filter ? `${count(visible.length)} / ` : ""}
          {meta ?? `${count(list.count)} × ${list.itemSize} B`}
        </span>
      </div>
      <div className="pane-tools">
        <input
          className="search"
          placeholder="Search by ID or name…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          spellCheck={false}
        />
      </div>
      <div className={"table-head " + grid}>
        {withIcons && <span />}
        <span>#</span>
        <span>ID</span>
        <span>Name</span>
      </div>
      <div className="scroll" ref={scrollRef} tabIndex={0} onKeyDown={onKeyDown}>
        {!rows && <div className="empty-note">Loading…</div>}
        {rows && visible.length === 0 && (
          <div className="empty-note">{list.count === 0 ? "This list is empty." : "No records match."}</div>
        )}
        <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
          {virtualizer.getVirtualItems().map((item) => {
            const row = visible[item.index];
            return (
              <div
                key={row.index}
                className={"record-row " + grid + (row.index === selected ? " active" : "")}
                style={{ transform: `translateY(${item.start}px)`, height: ROW_HEIGHT }}
                onClick={() => onSelect(row.index)}
                onDoubleClick={() => onOpen?.(row.index)}
              >
                {withIcons &&
                  (row.icon ? (
                    <img className="row-icon" src={icon!(row.icon)} alt="" loading="lazy" draggable={false} />
                  ) : (
                    <span />
                  ))}
                <span className="muted mono">{row.index}</span>
                <span className="mono">{row.id}</span>
                <span className={"truncate" + (row.name ? "" : " muted")}>{row.name || "—"}</span>
              </div>
            );
          })}
        </div>
      </div>
    </section>
  );
}
