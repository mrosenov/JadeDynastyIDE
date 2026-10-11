import { useEffect, useMemo, useState } from "react";
import { Check, Loader2, Search, X } from "lucide-react";
import { aiPolicyTranslation, applyAiPolicyTexts } from "../elements/api";
import { count } from "../elements/format";
import type { AiEditResult, AiTextUpdate, AiTranslationPreview } from "../elements/types";

type Group = AiTextUpdate["group"];
const GROUPS: Record<Group, string> = { talk: "Talk texts", name: "Trigger names", summon: "Summon names" };

interface Props {
  path: string;
  shown: number | null;
  onApplied: (result: AiEditResult, count: number) => void;
  onClose: () => void;
}

const keyOf = (update: AiTextUpdate) => `${update.policy}:${update.trigger}:${update.operation ?? "n"}`;

/** Takes texts over from another aipolicy.data or JSON export: the same policy and trigger IDs, the same operations. */
export function AiTranslate({ path, shown, onApplied, onClose }: Props) {
  const [preview, setPreview] = useState<AiTranslationPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [groups, setGroups] = useState<Set<Group>>(new Set(["talk", "name", "summon"]));
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");

  useEffect(() => {
    let cancelled = false;
    aiPolicyTranslation(path)
      .then((next) => { if (!cancelled) { setPreview(next); setPicked(new Set(next.updates.map(keyOf))); } })
      .catch((problem) => { if (!cancelled) setError(String(problem).replace(/^Error: /, "")); });
    return () => { cancelled = true; };
  }, [path]);

  const counts = useMemo(() => {
    const out: Record<Group, number> = { talk: 0, name: 0, summon: 0 };
    for (const update of preview?.updates ?? []) out[update.group]++;
    return out;
  }, [preview]);
  const rows = useMemo(() => {
    const text = query.trim().toLowerCase();
    return (preview?.updates ?? []).filter((update) => groups.has(update.group) && (!text || String(update.policyId) === text || update.current.toLowerCase().includes(text) || update.text.toLowerCase().includes(text)));
  }, [groups, preview, query]);
  const chosen = (preview?.updates ?? []).filter((update) => groups.has(update.group) && picked.has(keyOf(update)));

  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      onApplied(await applyAiPolicyTexts(chosen, shown), chosen.length);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  };

  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal npcgen-nearby-dialog gshop-text-update" role="dialog" aria-label="Take texts over" onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head"><h3>Take texts over</h3><span className="muted small mono truncate" title={path}>from {path.split(/[\\/]/).pop()}</span><span className="spacer" /><button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button></header>
      <div className="npcgen-nearby-bar">
        {(Object.keys(GROUPS) as Group[]).map((group) => <label key={group} className="npcgen-flag"><input type="checkbox" checked={groups.has(group)} onChange={(event) => setGroups((current) => { const next = new Set(current); if (event.target.checked) next.add(group); else next.delete(group); return next; })} /> {GROUPS[group]} <span className="muted">{count(counts[group])}</span></label>)}
        <span className="dyn-task-search ai-cmp-search"><Search size={13} /><input value={query} placeholder="Policy ID or text" onChange={(event) => setQuery(event.target.value)} /></span>
        <span className="spacer" />
        {preview && (preview.skippedTriggers > 0 || preview.tooLong > 0) && <span className="muted small">{preview.skippedTriggers > 0 && `${count(preview.skippedTriggers)} triggers skipped (other operations)`}{preview.skippedTriggers > 0 && preview.tooLong > 0 && "; "}{preview.tooLong > 0 && `${count(preview.tooLong)} texts too long to store`}</span>}
      </div>
      {error && <div className="path-data-message error">{error}</div>}
      <div className="npcgen-nearby-table">
        {!preview ? !error && <div className="empty-note"><Loader2 size={14} className="spin" /> Reading the other file…</div>
          : !rows.length ? <div className="empty-note">No texts differ.</div>
          : <table className="dyn-table gshop-text-table">
            <thead><tr>
              <th><input type="checkbox" checked={rows.every((row) => picked.has(keyOf(row)))} title="Select every row shown" onChange={(event) => setPicked((current) => { const next = new Set(current); for (const row of rows) { if (event.target.checked) next.add(keyOf(row)); else next.delete(keyOf(row)); } return next; })} /></th>
              <th>Where</th><th>Now</th><th>New</th>
            </tr></thead>
            <tbody>{rows.slice(0, 2000).map((row) => <tr key={keyOf(row)} className={picked.has(keyOf(row)) ? "selected" : undefined}>
              <td><input type="checkbox" checked={picked.has(keyOf(row))} onChange={() => setPicked((current) => { const next = new Set(current); const key = keyOf(row); if (next.has(key)) next.delete(key); else next.add(key); return next; })} /></td>
              <td className="nowrap small">Policy <span className="mono">{row.policyId}</span>, trigger <span className="mono">{row.triggerId}</span><div className="muted">{row.operation === null ? "name" : `operation ${row.operation + 1}`}</div></td>
              <td className="gshop-text-cell muted">{row.current || <i>(empty)</i>}</td>
              <td className="gshop-text-cell">{row.text}</td>
            </tr>)}</tbody>
          </table>}
        {rows.length > 2000 && <div className="empty-note">The first 2,000 of {count(rows.length)} rows are listed; the ticks of all of them count.</div>}
      </div>
      <footer className="npcgen-nearby-foot">
        <span className="muted small">Policies and triggers pair by ID; talk and summon texts only where both triggers have the same operations. One undo step.</span>
        <span className="spacer" />
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn primary" disabled={!chosen.length || busy} onClick={() => void apply()}>{busy ? <Loader2 size={14} className="spin" /> : <Check size={14} />} Take over {count(chosen.length)} text{chosen.length === 1 ? "" : "s"}</button>
      </footer>
    </div>
  </div>;
}
