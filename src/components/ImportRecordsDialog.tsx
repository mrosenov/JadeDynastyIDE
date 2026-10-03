import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { CircleAlert, FileUp, FolderOpen, Loader2, RefreshCw, X } from "lucide-react";
import { importRecords } from "../elements/api";
import { count } from "../elements/format";
import type { EditState, ImportReport } from "../elements/types";

interface Props {
  onApplied: (state: EditState) => void;
  onClose: () => void;
}

export function ImportRecordsDialog({ onApplied, onClose }: Props) {
  const [path, setPath] = useState("");
  const [plan, setPlan] = useState<ImportReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [applying, setApplying] = useState(false);
  const [tab, setTab] = useState<"changes" | "additions" | "issues">("changes");
  const active = useRef(true);
  const applyingCount = (plan?.changing ?? 0) + (plan?.adding ?? 0);

  useEffect(() => {
    active.current = true;
    return () => { active.current = false; };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) { e.preventDefault(); onClose(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, busy]);

  const preview = async (selected: string) => {
    setBusy(true);
    setPlan(null);
    setError(null);
    try {
      const result = await importRecords(selected);
      if (active.current) { setPlan(result); setTab(result.adding ? "additions" : result.changing ? "changes" : result.rejected ? "issues" : "changes"); }
    } catch (e) {
      if (active.current) setError(String(e));
    } finally {
      if (active.current) setBusy(false);
    }
  };

  const choose = async () => {
    setBusy(true);
    setError(null);
    try {
      const selected = await open({ multiple: false, filters: [{ name: "JD IDE JSON exports", extensions: ["json"] }] });
      if (typeof selected === "string" && active.current) { setPath(selected); await preview(selected); }
    } catch (e) {
      if (active.current) setError(String(e));
    } finally {
      if (active.current) setBusy(false);
    }
  };

  const apply = async () => {
    if (!plan || !applyingCount || busy) return;
    setBusy(true);
    setApplying(true);
    setError(null);
    try {
      const result = await importRecords(path, plan.token);
      if (result.state) onApplied(result.state);
      onClose();
    } catch (e) {
      setError(String(e));
      setPlan(null);
    } finally {
      setBusy(false);
      setApplying(false);
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <div className="modal import-records-dialog" role="dialog" aria-modal="true" aria-label="Import JSON">
        <header className="modal-head">
          <FileUp size={16} /><h3>Import JSON</h3><span className="spacer" />
          <button className="icon-btn" onClick={onClose} disabled={busy} aria-label="Close"><X size={18} /></button>
        </header>
        <div className="import-records-intro">
          <p>Import records from a JD IDE export. Records match by <b>list and ID</b>; keep their ID and list metadata unchanged.</p>
          <p className="muted small">New JSON exports can add missing records in bulk, using their complete source data. The elements version and list layouts must match. Older JSON exports update existing records only.</p>
          <div className="import-records-file">
            <button className="btn" onClick={choose} disabled={busy} autoFocus><FolderOpen size={14} /> Choose file…</button>
            <span className="truncate muted small" title={path}>{path || "JSON exported by JD IDE"}</span>
            {path && <button className="btn small" onClick={() => preview(path)} disabled={busy}><RefreshCw size={13} /> Refresh preview</button>}
          </div>
        </div>
        {error && <div className="se-problems import-records-error" role="alert"><CircleAlert size={14} /> {error}</div>}
        <div className="import-records-preview" aria-live="polite">
          {busy ? <p className="muted"><Loader2 size={14} className="spin" /> {applying ? "Applying import…" : "Reading and checking records…"}</p> : plan ? (
            <>
              <p className="muted small">{plan.sourceVersion !== null ? `Source: elements v${plan.sourceVersion} · version and layouts match · missing records will be added.` : "Update only: this file has no version metadata. Export again as JSON to transfer missing records."}</p>
              <div className="bulk-counts">
                <span>{count(plan.total)} input records · {count(plan.matched)} matched</span>
                <span className="diff-add"><b>{count(plan.adding)}</b> will be added</span>
                <span><b>{count(plan.changing)}</b> will be updated ({count(plan.fields)} field{plan.fields === 1 ? "" : "s"})</span>
                <span className="muted">{count(plan.unchanged)} unchanged</span>
                {plan.rejected > 0 && <span className="diff-del">{count(plan.rejected)} skipped</span>}
              </div>
              {plan.rejected > 0 && <p className="small">Rows with errors are skipped entirely. Only valid rows will be applied.</p>}
              <div className="segmented import-records-tabs" role="group" aria-label="Preview details">
                <button className={tab === "additions" ? "active" : ""} onClick={() => setTab("additions")}>Added ({count(plan.adding)})</button>
                <button className={tab === "changes" ? "active" : ""} onClick={() => setTab("changes")}>Updated ({count(plan.changing)})</button>
                <button className={tab === "issues" ? "active" : ""} onClick={() => setTab("issues")}>Skipped rows ({count(plan.rejected)})</button>
              </div>
              <div className="import-records-details scroll">
                {tab === "additions" ? plan.additions.length ? (
                  <table className="import-records-table">
                    <thead><tr><th>Input row</th><th>List</th><th>ID</th><th>Name</th></tr></thead>
                    <tbody>{plan.additions.map((a) => <tr key={a.sourceRow}>
                      <td>{a.sourceRow}</td><td>{a.list}</td><td className="diff-add">{a.id}</td><td>{a.name || "(unnamed)"}</td>
                    </tr>)}</tbody>
                  </table>
                ) : <p className="muted small">No new records to add.</p> : tab === "changes" ? plan.changes.length ? (
                  <table className="import-records-table">
                    <thead><tr><th>Input row · List · ID</th><th>Field</th><th>Current</th><th>Imported</th></tr></thead>
                    <tbody>{plan.changes.map((c, i) => <tr key={i}>
                      <td>{c.sourceRow} · {c.list} · {c.id}</td><td className="mono">{c.field}</td>
                      <td className="hf-old">{c.old || "(empty)"}</td><td className="hf-new">{c.new || "(empty)"}</td>
                    </tr>)}</tbody>
                  </table>
                ) : <p className="muted small">No field changes to apply.</p> : plan.issues.length ? plan.issues.map((issue) => (
                  <div className="import-records-issue" key={issue.sourceRow}><b>Input row {issue.sourceRow}</b><span>{issue.message}</span></div>
                )) : <p className="muted small">No rows have errors.</p>}
              </div>
              {tab === "changes" && plan.fields > plan.changes.length && <p className="muted small">Showing the first {count(plan.changes.length)} of {count(plan.fields)} field changes.</p>}
              {tab === "additions" && plan.adding > plan.additions.length && <p className="muted small">Showing the first {count(plan.additions.length)} of {count(plan.adding)} additions.</p>}
              {tab === "issues" && plan.rejected > plan.issues.length && <p className="muted small">Showing the first {count(plan.issues.length)} of {count(plan.rejected)} skipped rows.</p>}
            </>
          ) : !error && <p className="muted small">Choose a file to preview its changes. Nothing changes until you apply.</p>}
        </div>
        <footer className="modal-foot">
          <span className="muted small">One undo step · Save afterwards to write elements.data.</span><span className="spacer" />
          <button className="btn" onClick={onClose} disabled={busy}>Cancel</button>
          <button className="btn primary" onClick={apply} disabled={busy || !applyingCount}>
            <FileUp size={14} /> Apply {count(applyingCount)} record{applyingCount === 1 ? "" : "s"}
          </button>
        </footer>
      </div>
    </div>
  );
}
