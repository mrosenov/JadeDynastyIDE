import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { CircleAlert, FileUp, FolderOpen, Loader2, RefreshCw, X } from "lucide-react";
import { importTasksJson } from "../elements/api";
import { count } from "../elements/format";
import type { TaskEditState, TaskImportReport } from "../elements/types";

interface Props {
  /** Folder to start the file picker in. */
  defaultPath?: string;
  onApplied: (state: TaskEditState, report: TaskImportReport) => void;
  onClose: () => void;
}

/** Imports tasks from a JD IDE tasks.data JSON export: preview first, then one undo step. */
export function TaskImportDialog({ defaultPath, onApplied, onClose }: Props) {
  const [path, setPath] = useState("");
  const [plan, setPlan] = useState<TaskImportReport | null>(null);
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
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) { event.preventDefault(); onClose(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, busy]);

  const preview = async (selected: string) => {
    setBusy(true);
    setPlan(null);
    setError(null);
    try {
      const result = await importTasksJson(selected);
      if (active.current) { setPlan(result); setTab(result.adding ? "additions" : result.changing ? "changes" : result.rejected ? "issues" : "changes"); }
    } catch (problem) {
      if (active.current) setError(String(problem).replace(/^Error: /, ""));
    } finally {
      if (active.current) setBusy(false);
    }
  };

  const choose = async () => {
    setError(null);
    const selected = await open({ multiple: false, defaultPath, filters: [{ name: "JD IDE task exports", extensions: ["json"] }] });
    if (typeof selected === "string" && active.current) { setPath(selected); await preview(selected); }
  };

  const apply = async () => {
    if (!plan || !applyingCount || busy) return;
    setBusy(true);
    setApplying(true);
    setError(null);
    try {
      const result = await importTasksJson(path, plan.token);
      if (result.state) onApplied(result.state, result);
      onClose();
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
      setPlan(null);
    } finally {
      setBusy(false);
      setApplying(false);
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !busy && onClose()}>
      <div className="modal import-records-dialog" role="dialog" aria-modal="true" aria-label="Import tasks from JSON">
        <header className="modal-head">
          <FileUp size={16} /><h3>Import tasks from JSON</h3><span className="spacer" />
          <button className="icon-btn" onClick={onClose} disabled={busy} aria-label="Close"><X size={18} /></button>
        </header>
        <div className="import-records-intro">
          <p>Import tasks from a JD IDE tasks.data export. Tasks match by <b>task ID</b>; only the fields present in the file change, with the same checks as editing in the inspector.</p>
          <p className="muted small">Missing top-level tasks are added with all their subquests from the complete task data in the export, keeping their IDs. The task version and layout must match. Missing subquests cannot be added.</p>
          <div className="import-records-file">
            <button className="btn" onClick={() => void choose()} disabled={busy} autoFocus><FolderOpen size={14} /> Choose file…</button>
            <span className="truncate muted small" title={path}>{path || "JSON exported by the tasks.data editor"}</span>
            {path && <button className="btn small" onClick={() => void preview(path)} disabled={busy}><RefreshCw size={13} /> Refresh preview</button>}
          </div>
        </div>
        {error && <div className="se-problems import-records-error" role="alert"><CircleAlert size={14} /> {error}</div>}
        <div className="import-records-preview" aria-live="polite">
          {busy ? <p className="muted"><Loader2 size={14} className="spin" /> {applying ? "Applying import…" : "Reading and checking tasks…"}</p> : plan ? (
            <>
              <p className="muted small">Source: tasks.data v{plan.sourceVersion} · version and layout match.</p>
              <div className="bulk-counts">
                <span>{count(plan.total)} input tasks</span>
                <span className="diff-add"><b>{count(plan.adding)}</b> top-level task{plan.adding === 1 ? "" : "s"} will be added</span>
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
                    <thead><tr><th>Input row</th><th>ID</th><th>Name</th><th>Quests</th><th>Pack</th></tr></thead>
                    <tbody>{plan.additions.map((addition) => <tr key={addition.sourceRow}>
                      <td>{addition.sourceRow}</td><td className="diff-add">{addition.id}</td><td>{addition.name || "(unnamed)"}</td><td>{count(addition.tasks)}</td><td>tasks.data{addition.pack + 1}</td>
                    </tr>)}</tbody>
                  </table>
                ) : <p className="muted small">No new tasks to add.</p> : tab === "changes" ? plan.changes.length ? (
                  <table className="import-records-table">
                    <thead><tr><th>Input row · ID</th><th>Field</th><th>Current</th><th>Imported</th></tr></thead>
                    <tbody>{plan.changes.map((change, index) => <tr key={index}>
                      <td title={change.name}>{change.sourceRow} · {change.id}</td><td className="mono">{change.field}</td>
                      <td className="hf-old">{change.old || "(empty)"}</td><td className="hf-new">{change.new || "(empty)"}</td>
                    </tr>)}</tbody>
                  </table>
                ) : <p className="muted small">No field changes to apply.</p> : plan.issues.length ? plan.issues.map((issue) => (
                  <div className="import-records-issue" key={issue.sourceRow}><b>Input row {issue.sourceRow}{issue.id !== undefined ? ` · task ${issue.id}` : ""}</b><span>{issue.message}</span></div>
                )) : <p className="muted small">No rows have errors.</p>}
              </div>
              {tab === "changes" && plan.fields > plan.changes.length && <p className="muted small">Showing the first {count(plan.changes.length)} of {count(plan.fields)} field changes.</p>}
              {tab === "additions" && plan.adding > plan.additions.length && <p className="muted small">Showing the first {count(plan.additions.length)} of {count(plan.adding)} additions.</p>}
              {tab === "issues" && plan.rejected > plan.issues.length && <p className="muted small">Showing the first {count(plan.issues.length)} of {count(plan.rejected)} skipped rows.</p>}
            </>
          ) : !error && <p className="muted small">Choose a file to preview its changes. Nothing changes until you apply.</p>}
        </div>
        <footer className="modal-foot">
          <span className="muted small">One undo step · Save afterwards to write tasks.data.</span><span className="spacer" />
          <button className="btn" onClick={onClose} disabled={busy}>Cancel</button>
          <button className="btn primary" onClick={() => void apply()} disabled={busy || !applyingCount}>
            <FileUp size={14} /> Apply {count(applyingCount)} task{applyingCount === 1 ? "" : "s"}
          </button>
        </footer>
      </div>
    </div>
  );
}
