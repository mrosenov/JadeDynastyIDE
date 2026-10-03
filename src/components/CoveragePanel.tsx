import { useEffect, useMemo, useState } from "react";
import { Braces, Gauge, Loader2, Pencil, RefreshCw, X } from "lucide-react";
import { layoutCoverage } from "../elements/api";
import type { CoverageRow } from "../elements/types";
import { LAYOUT_LABEL, count } from "../elements/format";

interface Props {
  /** Bumped when layouts change, to scan again. */
  generation: unknown;
  onOpenList: (list: number) => void;
  onEditSchema: (list: number) => void;
  onClose: () => void;
}

type Sort = "worst" | "index" | "bytes";

const pct = (part: number, whole: number) => (whole ? (100 * part) / whole : 100);
const fmtPct = (p: number) => (p >= 99.95 ? "100%" : p < 0.05 && p > 0 ? "<0.1%" : `${p.toFixed(1)}%`);

/** How much of each list's records the layout describes. */
export function CoveragePanel({ generation, onOpenList, onEditSchema, onClose }: Props) {
  const [rows, setRows] = useState<CoverageRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [sort, setSort] = useState<Sort>("worst");
  const [hideComplete, setHideComplete] = useState(true);
  const [hideEmpty, setHideEmpty] = useState(true);
  const [query, setQuery] = useState("");

  const load = () => {
    setBusy(true);
    layoutCoverage()
      .then((r) => {
        setRows(r);
        setError(null);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setBusy(false));
  };
  useEffect(load, [generation]);

  // Weighted by record bytes: a big list counts for more than a tiny one.
  const totals = useMemo(() => {
    let all = 0;
    let described = 0;
    let placeholder = 0;
    for (const r of rows ?? []) {
      all += r.itemSize * r.count;
      described += r.described * r.count;
      placeholder += r.placeholder * r.count;
    }
    const complete = (rows ?? []).filter((r) => r.described === r.itemSize).length;
    return { all, described, placeholder, complete };
  }, [rows]);

  const q = query.trim().toLowerCase();
  const shown = useMemo(() => {
    const list = (rows ?? []).filter(
      (r) =>
        (!hideComplete || r.described < r.itemSize) &&
        (!hideEmpty || r.count > 0) &&
        (!q || r.name.toLowerCase().includes(q) || (r.structName ?? "").toLowerCase().includes(q) || String(r.index) === q),
    );
    const key: Record<Sort, (r: CoverageRow) => number> = {
      worst: (r) => pct(r.described, r.itemSize),
      index: (r) => r.index,
      // Undescribed bytes in the whole file: where schema work pays off most.
      bytes: (r) => -(r.itemSize - r.described) * r.count,
    };
    return [...list].sort((a, b) => key[sort](a) - key[sort](b) || a.index - b.index);
  }, [rows, hideComplete, hideEmpty, q, sort]);

  return (
    <section className="pane search-panel coverage-panel" onKeyDown={(e) => e.key === "Escape" && onClose()}>
      <div className="pane-head">
        <span className="pane-title">
          <Gauge size={14} /> Layout coverage
        </span>
        <span className="spacer" />
        <button className="link" onClick={load} disabled={busy} title="Measure again">
          {busy ? <Loader2 size={13} className="spin" /> : <RefreshCw size={13} />} Refresh
        </button>
        <button className="icon-btn small" onClick={onClose} title="Close (Esc)" aria-label="Close">
          <X size={15} />
        </button>
      </div>

      <div className="problems-tools">
        {rows && (
          <div className="coverage-total">
            <div className="coverage-big">{fmtPct(pct(totals.described, totals.all))}</div>
            <div>
              <div>of all record bytes are described by named fields</div>
              <div className="muted small">
                {fmtPct(pct(totals.placeholder, totals.all))} placeholders ("unknown_12") · {totals.complete} of {rows.length} lists fully described
              </div>
            </div>
          </div>
        )}
        {rows && <CoverageBar described={totals.described} placeholder={totals.placeholder} size={totals.all} large />}
        <div className="coverage-legend muted small">
          <span>
            <i className="cov-described" /> named fields
          </span>
          <span>
            <i className="cov-placeholder" /> placeholders
          </span>
          <span>
            <i className="cov-undefined" /> no field
          </span>
        </div>
        <div className="coverage-controls">
          <input className="sf-control" placeholder="Filter lists…" value={query} onChange={(e) => setQuery(e.target.value)} spellCheck={false} />
          <select className="sf-control" value={sort} onChange={(e) => setSort(e.target.value as Sort)} title="Order of the lists">
            <option value="worst">Least covered first</option>
            <option value="bytes">Most undescribed bytes first</option>
            <option value="index">List order</option>
          </select>
        </div>
        <div className="search-row">
          <label className="check">
            <input type="checkbox" checked={hideComplete} onChange={(e) => setHideComplete(e.target.checked)} /> Hide fully described
          </label>
          <label className="check">
            <input type="checkbox" checked={hideEmpty} onChange={(e) => setHideEmpty(e.target.checked)} /> Hide empty lists
          </label>
        </div>
        {error && <div className="se-problems">{error}</div>}
      </div>

      <div className="search-results scroll">
        {rows && shown.length === 0 && <div className="empty-note">{hideComplete ? "Every list shown is fully described." : "No lists match."}</div>}
        {shown.map((r) => (
          <div key={r.index} className="coverage-row">
            <button className="coverage-main" onClick={() => onOpenList(r.index)} title={`${r.structName ?? r.name} · open the list`}>
              <span className="coverage-name">
                <span className="mono muted small">{r.index}</span>
                <span className="truncate">{r.name}</span>
                {r.custom && (
                  <span className="custom-mark" title="Your schema">
                    <Pencil size={11} />
                  </span>
                )}
                <span className="spacer" />
                <span className={"coverage-pct" + (r.described === r.itemSize ? " full" : "")}>{fmtPct(pct(r.described, r.itemSize))}</span>
              </span>
              <CoverageBar described={r.described} placeholder={r.placeholder} size={r.itemSize} />
              <span className="coverage-meta muted small">
                <span className={`fit-dot fit-${r.fit}`} /> {LAYOUT_LABEL[r.fit]}
                {r.layoutId ? ` (${r.layoutId})` : ""} · {r.itemSize} B · {count(r.fields)} fields
                {r.placeholder > 0 && ` · ${r.placeholder} B placeholders`}
                {r.undefined > 0 && ` · ${r.undefined} B without a field`} · {count(r.count)} records
              </span>
            </button>
            <button className="icon-btn small" onClick={() => onEditSchema(r.index)} title="Open this list in the schema editor">
              <Braces size={14} />
            </button>
          </div>
        ))}
      </div>
    </section>
  );
}

function CoverageBar({ described, placeholder, size, large = false }: { described: number; placeholder: number; size: number; large?: boolean }) {
  const d = pct(described, size);
  const p = pct(placeholder, size);
  return (
    <span className={"coverage-bar" + (large ? " large" : "")} aria-hidden>
      <span className="cov-described" style={{ width: `${d}%` }} />
      <span className="cov-placeholder" style={{ width: `${p}%` }} />
    </span>
  );
}
