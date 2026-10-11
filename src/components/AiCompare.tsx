import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Copy, FolderOpen, Loader2, Search, X } from "lucide-react";
import { aiPolicyComparePolicy, copyAiPolicy } from "../elements/api";
import { count } from "../elements/format";
import type { AiCompareRow, AiCompareSummary, AiCopyReport, AiDiffStatus, AiEditResult, AiOperation, AiPolicyDiff, AiTrigger } from "../elements/types";

interface Props {
  summary: AiCompareSummary;
  /** The open file is for the newer server. */
  newer: boolean;
  describe: (operation: AiOperation) => ReactNode;
  /** The policy shown in the editor (kept shown after a copy). */
  shown: number | null;
  onCopied: (result: AiEditResult, report: AiCopyReport) => void;
  onChooseOther: () => void;
  onClose: () => void;
}

type Filter = "differ" | AiDiffStatus;
const STATUS: Record<AiDiffStatus, string> = { same: "same", changed: "changed", here: "only here", there: "only there" };
const PAGE = 300;
const plural = (n: number, one: string, many: string) => `${count(n)} ${n === 1 ? one : many}`;

function TriggerSide({ trigger, describe }: { trigger: AiTrigger | null; describe: (operation: AiOperation) => ReactNode }) {
  if (!trigger) return <div className="ai-cmp-side empty muted small">—</div>;
  return <div className="ai-cmp-side">
    <div><b>{trigger.name || <span className="muted">(no name)</span>}</b> <span className="tag ai-cat">{trigger.run ? "Sub" : trigger.category}</span>{!trigger.run && !trigger.active && <span className="muted small"> disabled at start</span>}{trigger.battleOnly && <span className="muted small"> · in combat only</span>}</div>
    <div className="mono small ai-cmp-expr">{trigger.expression}</div>
    <ol className="ai-cmp-ops">{trigger.operations.map((operation, index) => <li key={index}>{describe(operation)}</li>)}</ol>
  </div>;
}

/** The open aipolicy.data against another one (or a JSON export): policies and triggers by ID, and copying. */
export function AiCompare({ summary, newer, describe, shown, onCopied, onChooseOther, onClose }: Props) {
  const [filter, setFilter] = useState<Filter>("differ");
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<number | null>(null);
  const [diff, setDiff] = useState<AiPolicyDiff | null>(null);
  const [policies, setPolicies] = useState<Set<number>>(new Set());
  const [triggers, setTriggers] = useState<Map<number, Set<number>>>(new Map());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<AiCopyReport | null>(null);
  const [onlyDiffering, setOnlyDiffering] = useState(true);

  const counts = useMemo(() => {
    const out: Record<AiDiffStatus, number> = { same: 0, changed: 0, here: 0, there: 0 };
    for (const row of summary.rows) out[row.status]++;
    return out;
  }, [summary]);
  const rows = useMemo(() => {
    const text = query.trim();
    return summary.rows.filter((row) => (filter === "differ" ? row.status !== "same" : row.status === filter) && (!text || String(row.id).includes(text)));
  }, [filter, query, summary]);
  const byId = useMemo(() => new Map(summary.rows.map((row) => [row.id, row])), [summary]);
  // The first row of a new list is shown.
  useEffect(() => { if (selected === null || !rows.some((row) => row.id === selected)) setSelected(rows[0]?.id ?? null); }, [rows, selected]);
  useEffect(() => {
    if (selected === null) { setDiff(null); return; }
    let cancelled = false;
    aiPolicyComparePolicy(selected).then((next) => { if (!cancelled) setDiff(next); }).catch((problem) => { if (!cancelled) setError(String(problem).replace(/^Error: /, "")); });
    return () => { cancelled = true; };
  }, [selected, summary]);

  const copyable = (row: AiCompareRow) => row.there !== null && row.status !== "same";
  const togglePolicy = (id: number) => setPolicies((current) => { const next = new Set(current); if (next.has(id)) next.delete(id); else next.add(id); return next; });
  const toggleTrigger = (policy: number, trigger: number) => setTriggers((current) => {
    const next = new Map(current);
    const set = new Set(next.get(policy) ?? []);
    if (set.has(trigger)) set.delete(trigger);
    else set.add(trigger);
    if (set.size) next.set(policy, set);
    else next.delete(policy);
    return next;
  });
  const items: { id: number; triggers?: number[] }[] = [
    ...[...policies].map((id) => ({ id })),
    ...[...triggers].filter(([id]) => !policies.has(id) && byId.get(id)?.here !== null).map(([id, set]) => ({ id, triggers: [...set] })),
  ];
  const triggerCount = items.reduce((sum, item) => sum + (item.triggers?.length ?? 0), 0);
  const policyCount = items.filter((item) => !item.triggers).length;

  const copy = async () => {
    setBusy(true);
    setError(null);
    try {
      const { result, report } = await copyAiPolicy(items, shown);
      setPolicies(new Set());
      setTriggers(new Map());
      setReport(report);
      onCopied(result, report);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  };

  const row = selected !== null ? byId.get(selected) ?? null : null;
  const pairs = (diff?.triggers ?? []).filter((pair) => !onlyDiffering || pair.status !== "same");
  const fileName = summary.path.split(/[\\/]/).pop();

  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal ai-sim-dialog ai-compare-dialog" role="dialog" aria-label="Compare" onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head">
        <h3>Compare with <span className="mono" title={summary.path}>{fileName}</span></h3>
        <span className="tag">{summary.newer ? "newer server" : "2013 server"}</span>
        <span className="spacer" />
        <button className="btn small" onClick={onChooseOther}><FolderOpen size={13} /> Other file…</button>
        <button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button>
      </header>
      <div className="npcgen-nearby-bar">
        <span className="dyn-compare-tabs">
          <button className={"btn small" + (filter === "differ" ? " active" : "")} onClick={() => setFilter("differ")}>Differ <span className="muted">{count(counts.changed + counts.here + counts.there)}</span></button>
          {(["changed", "there", "here", "same"] as AiDiffStatus[]).map((status) => <button key={status} className={"btn small" + (filter === status ? " active" : "")} onClick={() => setFilter(status)}>{STATUS[status][0].toUpperCase() + STATUS[status].slice(1)} <span className="muted">{count(counts[status])}</span></button>)}
        </span>
        <span className="dyn-task-search ai-cmp-search"><Search size={13} /><input value={query} placeholder="Policy ID" onChange={(event) => setQuery(event.target.value)} /></span>
        <span className="spacer" />
        {!newer && summary.newer && <span className="muted small" title="Triggers with types the 2013 server does not read are skipped; version-12 triggers are stored as version 11">The open file is for the 2013 server: newer types are not copied.</span>}
      </div>
      {error && <div className="path-data-message error">{error}</div>}
      {report && <div className="path-data-message">Copied: {plural(report.policiesAdded, "policy", "policies")} added, {count(report.policiesReplaced)} replaced; {plural(report.triggersAdded, "trigger", "triggers")} added, {count(report.triggersReplaced)} replaced.{report.skipped.length > 0 && <> Skipped {count(report.skipped.length)}: {report.skipped.slice(0, 4).join("; ")}{report.skipped.length > 4 ? "; …" : ""}</>} <button className="link" onClick={() => setReport(null)}>Dismiss</button></div>}
      <div className="ai-cmp-body">
        <div className="ai-cmp-list">
          <div className="ai-cmp-list-head">
            <input type="checkbox" title="Select every policy in the list that can be copied" checked={rows.some(copyable) && rows.filter(copyable).every((entry) => policies.has(entry.id))} onChange={(event) => setPolicies((current) => { const next = new Set(current); for (const entry of rows.filter(copyable)) { if (event.target.checked) next.add(entry.id); else next.delete(entry.id); } return next; })} />
            <span className="muted small">{count(rows.length)} policies{rows.length > PAGE ? ` (first ${count(PAGE)} shown)` : ""}</span>
          </div>
          {rows.slice(0, PAGE).map((entry) => <div key={entry.id} className={"ai-cmp-row" + (entry.id === selected ? " selected" : "")} onClick={() => setSelected(entry.id)}>
            {copyable(entry) ? <input type="checkbox" checked={policies.has(entry.id)} onClick={(event) => event.stopPropagation()} onChange={() => togglePolicy(entry.id)} title="Copy the whole policy" /> : <span />}
            <span className="mono">{entry.id}</span>
            <span className={"tag ai-cmp-" + entry.status}>{STATUS[entry.status]}</span>
            <span className="muted small truncate">{entry.status === "changed" ? entry.reordered ? "triggers in another order" : [entry.changed && `${entry.changed} changed`, entry.added && `${entry.added} only there`, entry.removed && `${entry.removed} only here`].filter(Boolean).join(", ") : ""}{triggers.get(entry.id)?.size ? ` · ${triggers.get(entry.id)!.size} picked` : ""}</span>
          </div>)}
          {!rows.length && <div className="empty-note">No policy.</div>}
        </div>
        <div className="ai-cmp-detail">
          {!row || !diff ? <div className="empty-note">{row ? <Loader2 size={14} className="spin" /> : "Select a policy."}</div> : <>
            <div className="ai-cmp-detail-head">
              <b>Policy {row.id}</b> <span className={"tag ai-cmp-" + row.status}>{STATUS[row.status]}</span>
              <span className="spacer" />
              <label className="check"><input type="checkbox" checked={onlyDiffering} onChange={(event) => setOnlyDiffering(event.target.checked)} /> Only differing triggers</label>
            </div>
            {row.status === "here" ? <p className="muted small">Only the open file has this policy.</p> : row.here === null ? <p className="muted small">Copying adds the whole policy (in ID order).</p> : <p className="muted small">Tick a whole policy in the list to replace it, or single triggers below: a trigger replaces the one of its ID, or is added at the end.</p>}
            <div className="ai-cmp-pairs">
              <div className="ai-cmp-pair-head"><span /><span>Here</span><span>{fileName}</span></div>
              {pairs.map((pair, index) => {
                const mine = pair.here !== null ? diff.here?.triggers[pair.here] ?? null : null;
                const theirs = pair.there !== null ? diff.there?.triggers[pair.there] ?? null : null;
                const pickable = pair.status !== "same" && pair.status !== "here" && row.here !== null;
                return <div key={index} className={"ai-cmp-pair ai-cmp-" + pair.status}>
                  <div className="ai-cmp-pair-id">
                    {pickable ? <input type="checkbox" disabled={policies.has(row.id)} checked={policies.has(row.id) || !!triggers.get(row.id)?.has(pair.id)} onChange={() => toggleTrigger(row.id, pair.id)} title={pair.status === "there" ? "Add this trigger" : "Replace the trigger of this ID"} /> : <span />}
                    <span className="mono">{pair.id}</span>
                    <span className={"tag ai-cmp-" + pair.status}>{STATUS[pair.status]}</span>
                  </div>
                  <TriggerSide trigger={mine} describe={describe} />
                  <TriggerSide trigger={theirs} describe={describe} />
                </div>;
              })}
              {!pairs.length && <div className="empty-note">Every trigger is the same{row.reordered ? "; they are in another order" : ""}.</div>}
            </div>
          </>}
        </div>
      </div>
      <footer className="npcgen-nearby-foot">
        <span className="muted small">Trigger versions are ignored when comparing (the bytes are the same). One undo step.</span>
        <span className="spacer" />
        <button className="btn" onClick={onClose}>Close</button>
        <button className="btn primary" disabled={!items.length || busy} onClick={() => void copy()}>{busy ? <Loader2 size={14} className="spin" /> : <Copy size={14} />} Copy {policyCount ? `${count(policyCount)} polic${policyCount === 1 ? "y" : "ies"}` : ""}{policyCount && triggerCount ? " and " : ""}{triggerCount ? `${count(triggerCount)} trigger${triggerCount === 1 ? "" : "s"}` : ""}</button>
      </footer>
    </div>
  </div>;
}
