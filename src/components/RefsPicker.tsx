import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Check, Link2, Plus, Search, X } from "lucide-react";
import type { RefTarget } from "../elements/types";

interface Props {
  /** Struct names of the lists the field's IDs point at. */
  value: string[];
  targets: RefTarget[];
  disabled?: boolean;
  onChange: (refs: string[]) => void;
}

/** Shows chips that fit in the cell; the rest are counted. */
const VISIBLE_CHIPS = 2;

/**
 * Multi-select for a field's ref targets: chips for the chosen lists and a
 * searchable picker. A field can point at several lists (e.g. any item type).
 */
export function RefsPicker({ value, targets, disabled, onChange }: Props) {
  const [open, setOpen] = useState(false);
  const cell = useRef<HTMLDivElement>(null);

  if (disabled) return <span className="muted cell-static">—</span>;

  const known = new Set(targets.map((t) => t.structName));
  const remove = (s: string) => onChange(value.filter((v) => v !== s));

  return (
    <div className="refs-cell" ref={cell}>
      <div className="refs-chips" onClick={() => setOpen(true)} title={value.join(", ") || "Pick the lists this field's IDs point at"}>
        {value.slice(0, VISIBLE_CHIPS).map((s) => (
          <span key={s} className={"ref-chip" + (known.has(s) ? "" : " missing")} title={known.has(s) ? s : `${s} is not a list of this file`}>
            <span className="truncate">{s}</span>
            <button
              onClick={(e) => {
                e.stopPropagation();
                remove(s);
              }}
              aria-label={`Remove ${s}`}
            >
              <X size={11} />
            </button>
          </span>
        ))}
        {value.length > VISIBLE_CHIPS && <span className="ref-more">+{value.length - VISIBLE_CHIPS}</span>}
        <button className="ref-add" aria-label="Add a list" title="Add or remove lists">
          {value.length ? <Plus size={13} /> : (
            <>
              <Link2 size={12} /> refs
            </>
          )}
        </button>
      </div>
      {open && cell.current && (
        <RefsPopover
          anchor={cell.current.getBoundingClientRect()}
          value={value}
          targets={targets}
          onChange={onChange}
          onClose={() => setOpen(false)}
        />
      )}
    </div>
  );
}

function RefsPopover({
  anchor,
  value,
  targets,
  onChange,
  onClose,
}: {
  anchor: DOMRect;
  value: string[];
  targets: RefTarget[];
  onChange: (refs: string[]) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const box = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: anchor.left, top: anchor.bottom + 4 });

  // One row per struct (a struct may name several lists); chosen ones first.
  const rows = useMemo(() => {
    const byStruct = new Map<string, RefTarget[]>();
    for (const t of targets) byStruct.set(t.structName, [...(byStruct.get(t.structName) ?? []), t]);
    const q = query.trim().toLowerCase();
    const all = [...byStruct.entries()].map(([structName, lists]) => ({ structName, lists }));
    for (const s of value) if (!byStruct.has(s)) all.push({ structName: s, lists: [] });
    return all
      .filter(
        (r) =>
          !q ||
          r.structName.toLowerCase().includes(q) ||
          r.lists.some((l) => l.name.toLowerCase().includes(q) || String(l.list) === q),
      )
      .sort((a, b) => Number(value.includes(b.structName)) - Number(value.includes(a.structName)) || a.structName.localeCompare(b.structName));
  }, [targets, value, query]);

  const typed = query.trim().toUpperCase();
  const canAddTyped = /^[A-Z0-9_]+$/.test(typed) && !rows.some((r) => r.structName === typed);

  const toggle = (s: string) => onChange(value.includes(s) ? value.filter((v) => v !== s) : [...value, s]);

  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const left = Math.max(8, Math.min(anchor.left, window.innerWidth - width - 8));
    const below = anchor.bottom + 4;
    setPos({ left, top: below + height > window.innerHeight - 8 ? Math.max(8, anchor.top - height - 4) : below });
  }, [anchor]);

  useEffect(() => {
    const onDown = (e: MouseEvent) => !box.current?.contains(e.target as Node) && onClose();
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [onClose]);

  useEffect(() => setActive(0), [query]);

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(rows.length - 1, a + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(0, a - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (rows[active]) toggle(rows[active].structName);
      else if (canAddTyped) toggle(typed);
    }
  };

  return (
    <div className="refs-popover" ref={box} style={pos} onKeyDown={onKey}>
      <div className="refs-search">
        <Search size={14} />
        <input
          autoFocus
          className="search"
          placeholder="Search lists by struct, name or number…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          spellCheck={false}
        />
      </div>
      <div className="refs-list scroll">
        {rows.map((r, i) => {
          const on = value.includes(r.structName);
          return (
            <button
              key={r.structName}
              className={"refs-option" + (on ? " on" : "") + (i === active ? " active" : "")}
              onMouseEnter={() => setActive(i)}
              onClick={() => toggle(r.structName)}
            >
              <span className="refs-check">{on && <Check size={13} />}</span>
              <span className="mono truncate">{r.structName}</span>
              <span className="muted small truncate">
                {r.lists.length
                  ? r.lists.map((l) => `#${l.list} ${l.name}`).join(", ")
                  : "not a list of this file"}
              </span>
            </button>
          );
        })}
        {rows.length === 0 && !canAddTyped && <div className="empty-note">No list matches.</div>}
        {canAddTyped && (
          <button className="refs-option" onClick={() => toggle(typed)}>
            <span className="refs-check">
              <Plus size={13} />
            </span>
            <span className="mono truncate">{typed}</span>
            <span className="muted small">add a struct this file does not have</span>
          </button>
        )}
      </div>
      <div className="refs-foot">
        <span className="muted small">
          {value.length} selected · the field's IDs are looked up in each, in order
        </span>
        <span className="spacer" />
        {value.length > 0 && (
          <button className="link" onClick={() => onChange([])}>
            Clear
          </button>
        )}
        <button className="link" onClick={onClose}>
          Done
        </button>
      </div>
    </div>
  );
}
