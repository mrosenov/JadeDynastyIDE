import { useMemo, useState } from "react";
import type { ListSummary } from "../elements/types";
import { LAYOUT_LABEL, count } from "../elements/format";
import { MessagesSquare, Pencil } from "lucide-react";
import { DIALOGS } from "../tabs";

interface Props {
  lists: ListSummary[];
  selected: number | null;
  onSelect: (index: number) => void;
  /** NPC dialogs in the file (listed first, selected as DIALOGS). */
  talkCount: number;
}

export function ListSidebar({ lists, selected, onSelect, talkCount }: Props) {
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
          String(l.index) === q),
    );
  }, [lists, filter, hideEmpty]);
  const q = filter.trim().toLowerCase();
  const showDialogs = (!hideEmpty || talkCount > 0) && (!q || ["npc dialogs", "talk_proc", "dialogs"].some((n) => n.includes(q)));

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
        {showDialogs && (
          <button
            className={"list-item dialogs-item" + (selected === DIALOGS ? " active" : "") + (talkCount === 0 ? " empty" : "")}
            onClick={() => onSelect(DIALOGS)}
            title={"TALK_PROC\nThe NPC dialog block at the end of the file"}
          >
            <span />
            <span className="dialogs-icon">
              <MessagesSquare size={13} />
            </span>
            <span className="list-name">NPC Dialogs</span>
            <span className="list-count">{count(talkCount)}</span>
          </button>
        )}
        {visible.map((l) => (
          <button
            key={l.index}
            className={"list-item" + (l.index === selected ? " active" : "") + (l.count === 0 ? " empty" : "")}
            onClick={() => onSelect(l.index)}
            title={[
              l.structName ?? l.name,
              `${l.itemSize} bytes × ${count(l.count)}`,
              LAYOUT_LABEL[l.layout] + (l.layoutId ? ` (${l.layoutId})` : ""),
              l.custom ? "Edited in the schema editor" : null,
            ]
              .filter(Boolean)
              .join("\n")}
          >
            <span className={`fit-dot fit-${l.layout}`} />
            <span className="list-num">{l.index}</span>
            <span className={"list-name" + (l.layout === "borrowed" || l.layout === "grown" ? " approx" : "")}>
              {l.name}
            </span>
            <span className="list-count">
              {l.custom && (
                <span className="custom-mark" title="Your schema">
                  <Pencil size={11} />
                </span>
              )}
              {count(l.count)}
            </span>
          </button>
        ))}
        {visible.length === 0 && !showDialogs && <div className="empty-note">No lists match.</div>}
      </div>
    </aside>
  );
}
