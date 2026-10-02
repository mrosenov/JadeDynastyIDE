import { useEffect, useRef, useState } from "react";
import { CornerDownLeft, Hash, Search, X } from "lucide-react";
import { findRecords } from "../elements/api";
import type { FindHit, FindResult, ListSummary } from "../elements/types";

interface Props {
  lists: ListSummary[];
  /** The last search, shown again when the palette reopens. */
  initialQuery: string;
  icon?: (pathId?: number | null) => string | undefined;
  /** Open a hit: in the current tab, or a new one. */
  onOpen: (hit: FindHit, newTab: boolean) => void;
  /** The query and its hits, for stepping through them with F3 later. */
  onResults: (query: string, hits: FindHit[], position: number) => void;
  onClose: () => void;
}

/** The name with the query's first occurrence marked. */
function Marked({ name, query }: { name: string; query: string }) {
  const at = query ? name.toLowerCase().indexOf(query.toLowerCase()) : -1;
  if (at < 0) return <>{name}</>;
  return (
    <>
      {name.slice(0, at)}
      <mark>{name.slice(at, at + query.length)}</mark>
      {name.slice(at + query.length)}
    </>
  );
}

/** Find any record of the file by ID or name (Ctrl+G). */
export function FindPalette({ lists, initialQuery, icon, onOpen, onResults, onClose }: Props) {
  const [query, setQuery] = useState(initialQuery);
  const [result, setResult] = useState<FindResult | null>(null);
  const [active, setActive] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setResult(null);
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      findRecords(q)
        .then((r) => {
          if (cancelled) return;
          setResult(r);
          setActive(0);
        })
        .catch(() => !cancelled && setResult({ hits: [], total: 0 }));
    }, 80);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [query]);

  useEffect(() => {
    listRef.current?.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const hits = result?.hits ?? [];
  const open = (i: number, newTab: boolean) => {
    const hit = hits[i];
    if (!hit) return;
    onResults(query.trim(), hits, i);
    onOpen(hit, newTab);
    onClose();
  };

  const onKey = (e: React.KeyboardEvent) => {
    const step = { ArrowDown: 1, ArrowUp: -1, PageDown: 10, PageUp: -10 }[e.key];
    if (step && hits.length) {
      e.preventDefault();
      setActive((a) => Math.max(0, Math.min(hits.length - 1, a + step)));
    } else if (e.key === "Enter") {
      e.preventDefault();
      open(active, e.ctrlKey || e.metaKey);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    }
  };

  const numeric = /^\d+$/.test(query.trim());

  return (
    <div className="modal-backdrop find-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="find-palette" role="dialog" aria-label="Find a record">
        <div className="find-input">
          <Search size={16} />
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKey}
            onFocus={(e) => e.target.select()}
            placeholder="Find by ID or name in every list…"
            spellCheck={false}
          />
          <button className="icon-btn small" onClick={onClose} aria-label="Close">
            <X size={15} />
          </button>
        </div>

        {result && (
          <div className="find-results scroll" ref={listRef}>
            {hits.length === 0 && <div className="empty-note">No record has this {numeric ? "ID or name" : "name"}.</div>}
            {hits.map((h, i) => (
              <button
                key={`${h.list}:${h.index}`}
                data-i={i}
                className={"find-hit" + (i === active ? " active" : "")}
                onMouseMove={() => setActive(i)}
                onClick={(e) => open(i, e.ctrlKey || e.metaKey)}
                onAuxClick={(e) => e.button === 1 && open(i, true)}
              >
                <span className="find-icon">
                  {icon?.(h.icon) ? <img src={icon(h.icon)} alt="" draggable={false} /> : null}
                </span>
                <span className="find-name truncate">
                  {h.name ? <Marked name={h.name} query={h.how === "name" ? query.trim() : ""} /> : <span className="muted">Unnamed</span>}
                </span>
                <span className="find-list truncate muted">{lists[h.list]?.name ?? `List ${h.list}`}</span>
                <span className={"find-id mono" + (h.how === "id" ? " match" : "")} title={h.how === "id" ? "The ID matches" : undefined}>
                  {h.how === "id" && <Hash size={11} />}
                  {h.id}
                </span>
              </button>
            ))}
          </div>
        )}

        <div className="find-foot muted small">
          <span>
            <kbd>↑</kbd> <kbd>↓</kbd> choose · <kbd>
              <CornerDownLeft size={10} />
            </kbd>{" "}
            open · <kbd>Ctrl</kbd>+<kbd>
              <CornerDownLeft size={10} />
            </kbd>{" "}
            new tab · <kbd>F3</kbd> next result later
          </span>
          <span className="spacer" />
          {result && result.total > 0 && (
            <span>{result.total > hits.length ? `${hits.length} of ${result.total}` : `${result.total}`} found</span>
          )}
        </div>
      </div>
    </div>
  );
}
