import { useEffect, useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Check, FolderOpen, Loader2, X } from "lucide-react";
import { domainSevPreview, exportDomainSev } from "../elements/api";
import { count } from "../elements/format";
import type { DomainData, LandRow, SevDomain, SevPreview } from "../elements/types";

interface Props {
  /** domain1.data as it is open (unsaved changes included). */
  data: DomainData;
  /** configs\\land.txt of the configured client (null: none). */
  land: LandRow[] | null;
  /** Where domain1.data is, for the default target folder. */
  sourcePath: string;
  backup: boolean;
  onExported: (message: string) => void;
  onClose: () => void;
}

const TARGET_KEY = "jdide.domains.sevTarget";
const TYPES = ["None", "First class", "Second class", "Third class"];
const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
type Status = "same" | "changed" | "new" | "removed";

/** domain.sev for the territory war server (gdeliveryd), as the client's \`globalsave\` writes it. */
export function DomainSevExport({ data, land, sourcePath, backup, onExported, onClose }: Props) {
  const [target, setTarget] = useState<string>(() => { try { return localStorage.getItem(TARGET_KEY) ?? ""; } catch { return ""; } });
  const [officialMax, setOfficialMax] = useState(true);
  const [preview, setPreview] = useState<SevPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [onlyChanged, setOnlyChanged] = useState(false);

  useEffect(() => {
    let cancelled = false;
    domainSevPreview(data, land, officialMax ? 0 : data.timeMax, target || null)
      .then((next) => { if (!cancelled) setPreview(next); })
      .catch((problem) => { if (!cancelled) setError(String(problem).replace(/^Error: /, "")); });
    return () => { cancelled = true; };
  }, [data, land, officialMax, target]);

  const names = useMemo(() => {
    const out = new Map<number, string>();
    for (const domain of data.domains) if (!out.has(domain.id)) out.set(domain.id, domain.name);
    for (const row of land ?? []) out.set(row.id, row.name);
    return out;
  }, [data, land]);
  const rows = useMemo(() => {
    if (!preview) return [];
    const before = new Map((preview.existing?.domains ?? []).map((domain) => [domain.id, domain]));
    const same = (a: SevDomain, b: SevDomain) => a.type === b.type && a.lower === b.lower && a.upper === b.upper && a.neighbours.join(",") === b.neighbours.join(",");
    const out: { domain: SevDomain; old: SevDomain | null; status: Status }[] = preview.sev.domains.map((domain) => {
      const old = before.get(domain.id) ?? null;
      return { domain, old, status: !preview.existing ? "new" : !old ? "new" : same(domain, old) ? "same" : "changed" };
    });
    const ids = new Set(preview.sev.domains.map((domain) => domain.id));
    for (const old of preview.existing?.domains ?? []) if (!ids.has(old.id)) out.push({ domain: old, old, status: "removed" });
    return out;
  }, [preview]);
  const changed = rows.filter((row) => row.status !== "same").length;
  const shown = onlyChanged ? rows.filter((row) => row.status !== "same") : rows;
  const withoutLand = land ? preview?.sev.domains.filter((domain) => !land.some((row) => row.id === domain.id)).length ?? 0 : 0;
  const spaced = (land ?? []).filter((row) => row.spaceInName).length;

  const chooseTarget = async () => {
    const picked = await save({ defaultPath: target || `${sourcePath.replace(/[\\/][^\\/]*$/, "")}\\domain.sev`, title: "domain.sev for the server (gdeliveryd folder)", filters: [{ name: "domain.sev", extensions: ["sev"] }] });
    if (typeof picked !== "string") return;
    setTarget(picked);
    try { localStorage.setItem(TARGET_KEY, picked); } catch { /* optional */ }
  };
  const exportNow = async () => {
    if (!preview || !target) return;
    setBusy(true);
    setError(null);
    try {
      const report = await exportDomainSev(preview.sev, target, backup);
      onExported(`Wrote ${count(preview.sev.domains.length)} territories to ${target}.${report.backup ? ` Backup: ${report.backup}.` : ""} Restart gdeliveryd to load it.`);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  };

  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal npcgen-nearby-dialog gshop-text-update" role="dialog" aria-label="Export domain.sev" onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head"><h3>Export domain.sev for the server</h3><span className="muted small">the territory war server (gdeliveryd) reads it</span><span className="spacer" /><button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button></header>
      <div className="domain-sev-options">
        <div><b>From</b> domain1.data as open (unsaved changes included) {land ? <>and the client's configs\land.txt ({count(land.length)} rows: class, declaration item limits and neighbours){withoutLand > 0 && <span className="ai-warn">; {count(withoutLand)} territor{withoutLand === 1 ? "y has" : "ies have"} no row (the file's class and neighbours, no item limits)</span>}</> : <span className="ai-warn">without land.txt (no game client configured): the file's classes and neighbours, and no item limits</span>}.</div>
        {spaced > 0 && <div className="ai-warn small">{count(spaced)} land.txt name{spaced === 1 ? " has" : "s have"} spaces: the server file is right, but the client misreads those rows (see Problems).</div>}
        <div className="domain-inline"><b>To</b> <span className="mono truncate domain-sev-target" title={target}>{target || "(choose the server's domain.sev)"}</span><button className="btn small" onClick={() => void chooseTarget()}><FolderOpen size={13} /> Choose…</button></div>
        <div className="domain-inline">
          <b>Battles per slot</b>
          <label className="npcgen-flag"><input type="radio" checked={officialMax} onChange={() => setOfficialMax(true)} /> 0, as the official client writes it</label>
          <label className="npcgen-flag"><input type="radio" checked={!officialMax} onChange={() => setOfficialMax(false)} /> this file's value ({count(data.timeMax)})</label>
        </div>
        <div className="muted small">Battle times: {preview ? preview.sev.times.map((time) => `${DAYS[time[0]] ?? time[0]} ${time[1]}:${String(time[2]).padStart(2, "0")}`).join(", ") || "none" : "…"}. Both server sources replace the times and the battles per slot with fixed ones after reading, so only the territories matter to them.</div>
        {preview?.existingError && <div className="path-data-message error">The target is not a domain.sev ({preview.existingError}); it would be replaced.</div>}
        {preview?.same && <div className="path-data-message">The target already holds exactly this.</div>}
      </div>
      {error && <div className="path-data-message error">{error}</div>}
      <div className="npcgen-nearby-bar">
        <span className="muted small">{preview ? `${count(preview.sev.domains.length)} territories${preview.existing ? ` · ${count(changed)} differ from the target` : target ? " · the target does not exist yet" : ""}` : "…"}</span>
        <span className="spacer" />
        {preview?.existing && <label className="npcgen-flag"><input type="checkbox" checked={onlyChanged} onChange={(event) => setOnlyChanged(event.target.checked)} /> Only differences</label>}
      </div>
      <div className="npcgen-nearby-table">
        {!preview ? <div className="empty-note"><Loader2 size={14} className="spin" /> Building…</div> : <table className="dyn-table">
          <thead><tr><th>ID</th><th>Name</th><th>Class</th><th>Items</th><th>Neighbours</th>{preview.existing && <th />}</tr></thead>
          <tbody>{shown.map(({ domain, old, status }, index) => <tr key={index} className={status === "same" ? undefined : `domain-sev-${status}`}>
            <td className="mono">{domain.id}</td>
            <td>{names.get(domain.id) ?? ""}</td>
            <td>{TYPES[domain.type] ?? domain.type}{old && old.type !== domain.type && status === "changed" && <span className="muted small"> (was {TYPES[old.type] ?? old.type})</span>}</td>
            <td className="mono">{domain.lower}–{domain.upper}{old && status === "changed" && (old.lower !== domain.lower || old.upper !== domain.upper) && <span className="muted small"> (was {old.lower}–{old.upper})</span>}</td>
            <td className="mono small">{domain.neighbours.join(", ")}{old && status === "changed" && old.neighbours.join(",") !== domain.neighbours.join(",") && <div className="muted">was {old.neighbours.join(", ") || "none"}</div>}</td>
            {preview.existing && <td>{status === "same" ? <span className="muted small">same</span> : <span className="tag warn">{status}</span>}</td>}
          </tr>)}</tbody>
        </table>}
      </div>
      <footer className="npcgen-nearby-foot">
        <span className="muted small">Written like the client's <span className="mono">globalsave</span>; a backup of the replaced file is kept when Backup is on.</span>
        <span className="spacer" />
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn primary" disabled={!preview || !target || busy || preview.same} onClick={() => void exportNow()}>{busy ? <Loader2 size={14} className="spin" /> : <Check size={14} />} Export</button>
      </footer>
    </div>
  </div>;
}
