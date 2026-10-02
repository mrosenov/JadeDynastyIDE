import { useMemo, useState } from "react";
import type { ListSummary } from "../elements/types";
import { LAYOUT_LABEL, count } from "../elements/format";

interface Props {
  lists: ListSummary[];
  selected: number | null;
  onSelect: (index: number) => void;
}

export function ListSidebar({ lists, selected, onSelect }: Props) {
  const [filter, setFilter] = useState("");
  const [hideEmpty, setHideEmpty] = useState(false);

  const visible = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return lists.filter(
      (l) =>
        (!hideEmpty || l.count > 0) &&
        (!q ||
          l.name.toLowerCase().includes(q) ||
          l.key?.includes(q) ||
          l.structName?.toLowerCase().includes(q) ||
          String(l.index + 1) === q),
    );
  }, [lists, filter, hideEmpty]);

  return (
    <aside className="pane sidebar">
      <div className="pane-head">
        <span className="pane-title">Lists</span>
        <span className="muted">{lists.length}</span>
      </div>
      <div className="pane-tools">
        <input
          className="search"
          placeholder="Filter lists…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          spellCheck={false}
        />
        <label className="check" title="Hide lists with no records">
          <input type="checkbox" checked={hideEmpty} onChange={(e) => setHideEmpty(e.target.checked)} />
          Hide empty
        </label>
      </div>
      <div className="scroll">
        {visible.map((l) => (
          <button
            key={l.index}
            className={"list-item" + (l.index === selected ? " active" : "") + (l.count === 0 ? " empty" : "")}
            onClick={() => onSelect(l.index)}
            title={[
              l.structName ?? l.name,
              `${l.itemSize} bytes × ${count(l.count)}`,
              LAYOUT_LABEL[l.layout],
              l.approxName ? "Name borrowed by position from another version" : null,
            ]
              .filter(Boolean)
              .join("\n")}
          >
            <span className={`fit-dot fit-${l.layout}`} />
            <span className="list-num">{l.index + 1}</span>
            <span className={"list-name" + (l.approxName ? " approx" : "")}>{l.name}</span>
            <span className="list-count">{count(l.count)}</span>
          </button>
        ))}
        {visible.length === 0 && <div className="empty-note">No lists match.</div>}
      </div>
    </aside>
  );
}
