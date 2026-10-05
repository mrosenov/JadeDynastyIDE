import { useEffect, useRef, useState } from "react";
import { Check, ChevronLeft, ChevronRight, Search, X } from "lucide-react";
import type { FieldNode, PickerResult } from "../elements/types";
import { searchPicker } from "../elements/api";
import { GameText } from "./GameText";

interface Props {
  list: number;
  row: number;
  node: FieldNode;
  icon?: (pathId?: number | null) => string | undefined;
  onApply: (value: string) => Promise<string | null>;
  onClose: () => void;
}

export function ValuePicker({ list, row, node, icon, onApply, onClose }: Props) {
  const input = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(0);
  const [result, setResult] = useState<PickerResult | null>(null);
  const [active, setActive] = useState(0);
  const [loading, setLoading] = useState(true);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => input.current?.focus(), []);
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    const timer = window.setTimeout(() => {
      searchPicker({ list, row, off: node.off, query, page })
        .then((resultPage) => {
          if (cancelled) return;
          setResult(resultPage);
          const current = resultPage.entries.findIndex((entry) => entry.value === resultPage.current);
          setActive(current >= 0 && !query.trim() ? current : 0);
        })
        .catch((reason) => !cancelled && setError(String(reason)))
        .finally(() => !cancelled && setLoading(false));
    }, 90);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [list, row, node.off, query, page]);

  const choice = result?.entries[active];
  const pageCount = result ? Math.max(1, Math.ceil(result.total / result.pageSize)) : 1;
  const apply = async (value: number) => {
    if (applying) return;
    setApplying(true);
    setError(null);
    const problem = await onApply(String(value));
    setApplying(false);
    if (problem) setError(problem);
    else onClose();
  };
  const move = (amount: number) => {
    if (!result?.entries.length) return;
    setActive((index) => Math.max(0, Math.min(result.entries.length - 1, index + amount)));
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <div
        className="modal value-picker-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={result?.title ?? `Choose ${node.name}`}
        onKeyDown={(event) => {
          if (event.key === "Escape") onClose();
          else if (event.key === "ArrowDown") {
            event.preventDefault();
            move(1);
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            move(-1);
          } else if (event.key === "PageDown") {
            event.preventDefault();
            move(10);
          } else if (event.key === "PageUp") {
            event.preventDefault();
            move(-10);
          } else if (event.key === "Enter" && choice) {
            event.preventDefault();
            void apply(choice.value);
          }
        }}
      >
        <div className="modal-head">
          <div>
            <h3>{result?.title ?? `Choose ${node.name}`}</h3>
            <div className="muted small">{result?.scope ?? "Finding available values…"}</div>
          </div>
          <span className="spacer" />
          <button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button>
        </div>
        <div className="value-picker-search">
          <Search size={15} />
          <input
            ref={input}
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setPage(0);
            }}
            placeholder="Search by ID or name…"
            aria-label="Search available values"
          />
          {loading && <span className="muted small">Searching…</span>}
        </div>
        {error ? (
          <div className="value-picker-message error">{error}</div>
        ) : (
          <div className="value-picker-body">
            <div className="value-picker-results scroll" role="listbox" aria-label="Available values">
              {result?.entries.map((entry, index) => (
                <button
                  key={`${entry.value}:${entry.list ?? "resource"}:${entry.row ?? index}`}
                  className={"value-picker-row" + (index === active ? " active" : "")}
                  role="option"
                  aria-selected={index === active}
                  onMouseEnter={() => setActive(index)}
                  onClick={() => setActive(index)}
                  onDoubleClick={() => void apply(entry.value)}
                >
                  {entry.icon !== undefined && icon?.(entry.icon) ? <img src={icon(entry.icon)} alt="" draggable={false} /> : <span className="value-picker-icon-space" />}
                  <span className="value-picker-name truncate">
                    <strong className="truncate">{entry.name}</strong>
                    {entry.detail && <span className="muted truncate">{entry.detail}</span>}
                  </span>
                  <span className="mono">{entry.value}</span>
                  {entry.value === result.current && <Check size={14} className="value-picker-current" aria-label="Current value" />}
                </button>
              ))}
              {!loading && result?.entries.length === 0 && <div className="empty-note center">No matching values.</div>}
            </div>
            <div className="value-picker-preview">
              {choice ? (
                <>
                  <div className="value-picker-preview-head">
                    <strong>{choice.name}</strong>
                    <span className="mono">ID {choice.value}</span>
                  </div>
                  {choice.detail && <div className="muted small">{choice.detail}</div>}
                  {choice.description ? <GameText text={choice.description} /> : <div className="muted value-picker-no-description">No description is available.</div>}
                </>
              ) : (
                <div className="empty-note center">Select a value to preview it.</div>
              )}
            </div>
          </div>
        )}
        <div className="modal-foot">
          <button className="btn" disabled={applying} onClick={() => void apply(0)}>None / 0</button>
          <span className="muted small">
            {result ? `${result.total.toLocaleString()} match${result.total === 1 ? "" : "es"}` : ""}
          </span>
          <span className="spacer" />
          {result && pageCount > 1 && (
            <div className="value-picker-pages" aria-label="Result pages">
              <button className="icon-btn small" disabled={loading || result.page === 0} onClick={() => setPage((value) => Math.max(0, value - 1))} aria-label="Previous page">
                <ChevronLeft size={15} />
              </button>
              <span className="muted small">Page {result.page + 1} of {pageCount}</span>
              <button className="icon-btn small" disabled={loading || result.page + 1 >= pageCount} onClick={() => setPage((value) => value + 1)} aria-label="Next page">
                <ChevronRight size={15} />
              </button>
            </div>
          )}
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn primary" disabled={!choice || applying} onClick={() => choice && void apply(choice.value)}>
            {applying ? "Applying…" : "Use selected"}
          </button>
        </div>
      </div>
    </div>
  );
}
