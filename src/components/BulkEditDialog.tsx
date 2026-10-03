import { useEffect, useMemo, useState } from "react";
import { ArrowRight, CircleAlert, Layers, Loader2, Wand2, X } from "lucide-react";
import { bulkEdit, namedSet } from "../elements/api";
import type { BulkOp, BulkReport, EditState, ListSummary, SearchFieldName, SearchQuery, SetDetail } from "../elements/types";
import { count } from "../elements/format";

interface Props {
  /** The search whose matches are edited. */
  query: SearchQuery;
  matched: number;
  /** The results picked ([list, row]); the edit can be limited to them. */
  picked: [number, number][];
  names: SearchFieldName[];
  lists: ListSummary[];
  /** The field to start with (e.g. the first condition's). */
  initialField?: string;
  onApplied: (state: EditState) => void;
  onClose: () => void;
}

const OPS: { op: BulkOp; label: string; kinds: SearchFieldName["kind"][]; needsSet?: "mask" }[] = [
  { op: "set", label: "Set to", kinds: ["int", "float", "text", "bytes"] },
  { op: "add", label: "Add", kinds: ["int", "float"] },
  { op: "subtract", label: "Subtract", kinds: ["int", "float"] },
  { op: "multiply", label: "Multiply by", kinds: ["int", "float"] },
  { op: "set_flags", label: "Add flags (+=)", kinds: ["int"] },
  { op: "clear_flags", label: "Remove flags (−=)", kinds: ["int"] },
];

/** Changes one field of every search result at once, after a preview. */
export function BulkEditDialog({ query, matched, picked, names, lists, initialField = "", onApplied, onClose }: Props) {
  const [field, setField] = useState(initialField);
  const [op, setOp] = useState<BulkOp>("set");
  const [value, setValue] = useState("");
  const [plan, setPlan] = useState<BulkReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [applying, setApplying] = useState(false);
  const [set, setSet] = useState<SetDetail | null>(null);
  // Which records: the picked ones (when there are) or every result.
  const [scope, setScope] = useState<"picked" | "all">(picked.length ? "picked" : "all");
  const records = scope === "picked" ? picked : undefined;
  const target = scope === "picked" ? picked.length : matched;

  const info = useMemo(() => names.find((n) => n.name === field.trim().toLowerCase()), [names, field]);
  const ops = OPS.filter((o) => !info || o.kinds.includes(info.kind));
  const isMask = set?.kind === "mask";
  const labels = (set?.set.values ?? set?.set.flags ?? []).map((v) => v.label);

  useEffect(() => {
    setSet(null);
    if (info?.set) namedSet(info.set).then(setSet).catch(() => {});
  }, [info?.set]);

  // Masks start with "add flags"; an operation the field cannot take falls back to "set".
  useEffect(() => {
    if (isMask && op === "set") setOp("set_flags");
    else if (!ops.some((o) => o.op === op)) setOp("set");
  }, [isMask, info?.kind]);

  // The preview follows the inputs.
  useEffect(() => {
    setPlan(null);
    setError(null);
    if (!field.trim() || !value.trim()) return;
    let cancelled = false;
    setBusy(true);
    const timer = setTimeout(() => {
      bulkEdit({ query, records, field, op, value }, false)
        .then((r) => !cancelled && setPlan(r))
        .catch((e) => !cancelled && setError(String(e).replace(/^Error: /, "")))
        .finally(() => !cancelled && setBusy(false));
    }, 250);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [field, op, value, query, scope]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !applying && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, applying]);

  const apply = async () => {
    if (!plan?.changing) return;
    setApplying(true);
    try {
      const done = await bulkEdit({ query, records, field, op, value }, true);
      if (done.state) onApplied(done.state);
      onClose();
    } catch (e) {
      setError(String(e).replace(/^Error: /, ""));
    } finally {
      setApplying(false);
    }
  };

  const changes = plan?.samples.filter((s) => !s.error) ?? [];
  const failures = plan?.samples.filter((s) => s.error) ?? [];

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !applying && onClose()}>
      <div className="modal bulk-dialog" role="dialog" aria-label="Bulk edit">
        <header className="modal-head">
          <Layers size={16} />
          <h3>
            Bulk edit {count(target)} {scope === "picked" ? "picked " : ""}result{target === 1 ? "" : "s"}
          </h3>
          <span className="spacer" />
          <button className="icon-btn" onClick={onClose} aria-label="Close" disabled={applying}>
            <X size={18} />
          </button>
        </header>

        <div className="bulk-scope segmented" role="radiogroup" aria-label="Records">
          <button className={scope === "picked" ? "active" : ""} onClick={() => setScope("picked")} disabled={!picked.length} title={picked.length ? "Only the results you picked" : "Pick results in the list first"}>
            Picked ({count(picked.length)})
          </button>
          <button className={scope === "all" ? "active" : ""} onClick={() => setScope("all")} title="Every result of the search, also those past the 500 shown">
            All results ({count(matched)})
          </button>
        </div>
        <div className="bulk-form">
          <datalist id="bulk-fields">
            {names.slice(0, 2000).map((n) => (
              <option key={n.name} value={n.name}>
                {n.kind}
                {n.set ? ` · ${n.set}` : ""}
              </option>
            ))}
          </datalist>
          <label className="sf-field">
            <span className="sf-label">Field</span>
            <input className="sf-control mono" list="bulk-fields" value={field} onChange={(e) => setField(e.target.value)} placeholder="e.g. proc_type, or addons[2].id" spellCheck={false} autoFocus={!initialField} />
            {info && (
              <span className="sf-hint">
                <span className={`sf-kind ${info.kind}`}>{info.kind}</span>
                {info.set && <span className="sf-set">{info.set}</span>}
              </span>
            )}
          </label>
          <label className="sf-field">
            <span className="sf-label">Change</span>
            <select className="sf-control" value={op} onChange={(e) => setOp(e.target.value as BulkOp)}>
              {ops.map((o) => (
                <option key={o.op} value={o.op}>
                  {o.label}
                </option>
              ))}
            </select>
          </label>
          <label className="sf-field">
            <span className="sf-label">Value</span>
            <input
              className="sf-control"
              list={labels.length ? "bulk-labels" : undefined}
              value={value}
              onChange={(e) => setValue(e.target.value)}
              placeholder={op === "set_flags" || op === "clear_flags" ? "labels | joined, or a number" : set ? "number or label" : "value"}
              spellCheck={false}
              autoFocus={!!initialField}
            />
            {labels.length > 0 && (
              <datalist id="bulk-labels">
                {labels.map((l) => (
                  <option key={l} value={l} />
                ))}
              </datalist>
            )}
          </label>
        </div>

        <div className="bulk-preview">
          {!field.trim() || !value.trim() ? (
            <div className="muted small">Pick a field and a value to see what would change. Nothing changes until you apply.</div>
          ) : busy && !plan ? (
            <div className="muted small">
              <Loader2 size={13} className="spin" /> Working out the changes…
            </div>
          ) : error ? (
            <div className="se-problems">
              <CircleAlert size={13} /> {error}
            </div>
          ) : plan ? (
            <>
              <div className="bulk-counts">
                <span className="diff-add">
                  <b>{count(plan.changing)}</b> will change
                </span>
                {plan.unchanged > 0 && <span className="muted">{count(plan.unchanged)} already hold it</span>}
                {plan.failed > 0 && (
                  <span className="diff-del" title="The new value does not fit these records' field; they are left as they are">
                    {count(plan.failed)} can't take it
                  </span>
                )}
                {plan.skipped > 0 && (
                  <span className="muted" title={`Lists without the field: ${plan.skippedLists.join(", ")}`}>
                    {count(plan.skipped)} skipped (no such field in {plan.skippedLists.length} list{plan.skippedLists.length === 1 ? "" : "s"})
                  </span>
                )}
              </div>
              <div className="bulk-samples scroll">
                {changes.map((s) => (
                  <div key={`${s.list}:${s.row}`} className="bulk-sample">
                    <span className="truncate">
                      {s.name || `#${s.row}`} <span className="muted small">· {lists[s.list]?.name ?? `List ${s.list}`} · {s.id}</span>
                    </span>
                    <span className="hf-old" title={s.oldLabel}>
                      {s.old || "(empty)"}
                      {s.oldLabel && <span className="bulk-label"> {s.oldLabel}</span>}
                    </span>
                    <ArrowRight size={12} className="hf-arrow" />
                    <span className="hf-new" title={s.newLabel}>
                      {s.new || "(empty)"}
                      {s.newLabel && <span className="bulk-label"> {s.newLabel}</span>}
                    </span>
                  </div>
                ))}
                {plan.changing > changes.length && <div className="muted small bulk-more">and {count(plan.changing - changes.length)} more</div>}
                {failures.map((s) => (
                  <div key={`f${s.list}:${s.row}`} className="bulk-sample failed">
                    <span className="truncate">
                      {s.name || `#${s.row}`} <span className="muted small">· {s.id}</span>
                    </span>
                    <span className="diff-del small">{s.error}</span>
                  </div>
                ))}
              </div>
            </>
          ) : null}
        </div>

        <footer className="modal-foot">
          <span className="muted small">One undo step (Ctrl+Z) takes it all back.</span>
          <span className="spacer" />
          <button className="btn" onClick={onClose} disabled={applying}>
            Cancel
          </button>
          <button className="btn primary" onClick={apply} disabled={!plan?.changing || applying || busy}>
            {applying ? <Loader2 size={14} className="spin" /> : <Wand2 size={14} />} Apply to {count(plan?.changing ?? 0)} record{plan?.changing === 1 ? "" : "s"}
          </button>
        </footer>
      </div>
    </div>
  );
}
