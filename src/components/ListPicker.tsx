import { forwardRef, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { ChevronDown, MessagesSquare, Pencil, Search } from "lucide-react";
import type { ListSummary } from "../elements/types";
import { LAYOUT_LABEL, count } from "../elements/format";
import { DIALOGS } from "../tabs";

interface Props {
  lists: ListSummary[];
  /** The list shown (DIALOGS for the NPC dialogs, null for none). */
  selected: number | null;
  onSelect: (index: number) => void;
  /** NPC dialogs in the file (offered first). */
  talkCount: number;
  /** Records with unsaved edits, per list. */
  changedCounts?: Map<number, number>;
}

export interface ListPickerHandle {
  open: () => void;
}

interface Entry {
  index: number;
  name: string;
  count: number;
  list?: ListSummary;
}

/** The list to browse: a button showing it, a searchable dropdown to pick another. */
export const ListPicker = forwardRef<ListPickerHandle, Props>(function ListPicker({ lists, selected, onSelect, talkCount, changedCounts }, ref) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState("");
  const [hideEmpty, setHideEmpty] = useState(false);
  const [active, setActive] = useState(0);
  const box = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useImperativeHandle(ref, () => ({ open: () => setOpen(true) }), []);

  const entries: Entry[] = useMemo(
    () => [{ index: DIALOGS, name: "NPC Dialogs", count: talkCount }, ...lists.map((l) => ({ index: l.index, name: l.name, count: l.count, list: l }))],
    [lists, talkCount],
  );

  const visible = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return entries.filter((e) => {
      if (hideEmpty && e.count === 0) return false;
      if (!q) return true;
      if (e.index === DIALOGS) return ["npc dialogs", "talk_proc", "dialogs"].some((n) => n.includes(q));
      const l = e.list!;
      return l.name.toLowerCase().includes(q) || l.key?.includes(q) || l.structName?.toLowerCase().includes(q) || String(l.index) === q;
    });
  }, [entries, filter, hideEmpty]);

  // Opening: start at the current list.
  useEffect(() => {
    if (!open) return;
    setFilter("");
    const at = entries.findIndex((e) => e.index === selected);
    setActive(Math.max(0, at));
  }, [open]);
  useEffect(() => setActive((a) => Math.min(a, Math.max(0, visible.length - 1))), [visible.length]);
  useEffect(() => {
    if (open) listRef.current?.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active, open]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => !box.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  const pick = (index: number) => {
    setOpen(false);
    onSelect(index);
  };

  const onKey = (e: React.KeyboardEvent) => {
    const step = { ArrowDown: 1, ArrowUp: -1, PageDown: 12, PageUp: -12 }[e.key];
    if (step) {
      e.preventDefault();
      setActive((a) => Math.max(0, Math.min(visible.length - 1, a + step)));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (visible[active]) pick(visible[active].index);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      setOpen(false);
    }
  };

  const current = entries.find((e) => e.index === selected) ?? null;

  const row = (e: Entry) =>
    e.index === DIALOGS ? (
      <>
        <span />
        <span className="dialogs-icon">
          <MessagesSquare size={13} />
        </span>
        <span className="list-name">NPC Dialogs</span>
        <span className="list-count">{count(e.count)}</span>
      </>
    ) : (
      <>
        <span className={`fit-dot fit-${e.list!.layout}`} />
        <span className="list-num">{e.index}</span>
        <span className={"list-name" + (e.list!.layout === "borrowed" || e.list!.layout === "grown" ? " approx" : "")}>{e.name}</span>
        <span className="list-count">
          {!!changedCounts?.get(e.index) && (
            <span className="changed-count" title={`${changedCounts.get(e.index)} changed record(s), not saved yet`}>
              ●{changedCounts.get(e.index)}
            </span>
          )}
          {e.list!.custom && (
            <span className="custom-mark" title="Your schema">
              <Pencil size={11} />
            </span>
          )}
          {count(e.count)}
        </span>
      </>
    );

  return (
    <div className="list-picker" ref={box}>
      <button
        className={"list-picker-button" + (open ? " open" : "")}
        onClick={() => setOpen((o) => !o)}
        title={current?.list ? `${current.list.structName ?? current.name} · ${LAYOUT_LABEL[current.list.layout]} (Ctrl+L to pick another list)` : "Pick a list (Ctrl+L)"}
        aria-haspopup="listbox"
        aria-expanded={open}
      >
        {current ? <span className="list-item list-picker-current">{row(current)}</span> : <span className="muted">Pick a list…</span>}
        <ChevronDown size={15} className="list-picker-caret" />
      </button>
      {open && (
        <div className="list-picker-pop" onKeyDown={onKey}>
          <div className="list-picker-tools">
            <span className="search-box">
              <Search size={14} />
              <input
                className="search"
                autoFocus
                placeholder="Search lists by name, struct or number…"
                value={filter}
                onChange={(e) => {
                  setFilter(e.target.value);
                  setActive(0);
                }}
                spellCheck={false}
              />
            </span>
            <label className="check" title="Hide lists with no records">
              <input type="checkbox" checked={hideEmpty} onChange={(e) => setHideEmpty(e.target.checked)} /> Hide empty
            </label>
          </div>
          <div className="list-picker-list scroll" ref={listRef} role="listbox">
            {visible.map((e, i) => (
              <button
                key={e.index}
                data-i={i}
                role="option"
                aria-selected={e.index === selected}
                className={
                  "list-item" +
                  (e.index === selected ? " active" : "") +
                  (i === active ? " hover" : "") +
                  (e.count === 0 ? " empty" : "") +
                  (e.index === DIALOGS ? " dialogs-item" : "")
                }
                onMouseMove={() => setActive(i)}
                onClick={() => pick(e.index)}
                title={
                  e.list
                    ? [e.list.structName ?? e.name, `${e.list.itemSize} bytes × ${count(e.count)}`, LAYOUT_LABEL[e.list.layout] + (e.list.layoutId ? ` (${e.list.layoutId})` : ""), e.list.custom ? "Edited in the schema editor" : null]
                        .filter(Boolean)
                        .join("\n")
                    : "TALK_PROC: the NPC dialog block at the end of the file"
                }
              >
                {row(e)}
              </button>
            ))}
            {visible.length === 0 && <div className="empty-note">No lists match.</div>}
          </div>
          <div className="list-picker-foot muted small">
            {visible.length} of {entries.length} · <kbd>↑</kbd> <kbd>↓</kbd> <kbd>Enter</kbd>
          </div>
        </div>
      )}
    </div>
  );
});
