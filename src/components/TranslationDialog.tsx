import { useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Check, CircleAlert, FolderOpen, Languages, Loader2, RefreshCw, X } from "lucide-react";
import { applyTranslation, previewTranslation } from "../elements/api";
import { count } from "../elements/format";
import type { EditState, TranslationReport } from "../elements/types";

interface Props {
  onApplied: (state: EditState) => void;
  onClose: () => void;
}

const fileName = (path: string) => path.split(/[\\/]/).pop() || path;

export function TranslationDialog({ onApplied, onClose }: Props) {
  const [path, setPath] = useState("");
  const [plan, setPlan] = useState<TranslationReport | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [tab, setTab] = useState<"lists" | "changes" | "issues">("lists");
  const [busy, setBusy] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const active = useRef(true);

  useEffect(() => {
    active.current = true;
    return () => { active.current = false; };
  }, []);
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [busy, onClose]);

  const preview = async (sourcePath: string) => {
    setBusy(true);
    setPlan(null);
    setError(null);
    try {
      const result = await previewTranslation(sourcePath);
      if (!active.current) return;
      setPlan(result);
      setSelected(new Set(result.lists.filter((list) => list.fieldChanges > 0).map((list) => list.list)));
      setTab("lists");
    } catch (problem) {
      if (active.current) setError(String(problem));
    } finally {
      if (active.current) setBusy(false);
    }
  };

  const choose = async () => {
    setError(null);
    try {
      const picked = await open({ multiple: false, directory: false, title: "Choose the translated elements.data", filters: [{ name: "elements.data", extensions: ["data"] }] });
      if (typeof picked === "string" && active.current) {
        setPath(picked);
        await preview(picked);
      }
    } catch (problem) {
      if (active.current) setError(String(problem));
    }
  };

  const selectedReports = useMemo(() => plan?.lists.filter((list) => selected.has(list.list)) ?? [], [plan, selected]);
  const selectedRecords = selectedReports.reduce((sum, list) => sum + list.changedRecords, 0);
  const selectedFields = selectedReports.reduce((sum, list) => sum + list.fieldChanges, 0);
  const changeRows = plan?.changes.filter((change) => selected.has(change.list)) ?? [];
  const selectable = plan?.lists.filter((list) => list.fieldChanges > 0) ?? [];
  const allSelected = selectable.length > 0 && selectable.every((list) => selected.has(list.list));

  const apply = async () => {
    if (!plan || !selected.size || busy) return;
    setBusy(true);
    setApplying(true);
    setError(null);
    try {
      const state = await applyTranslation(path, plan.token, [...selected]);
      onApplied(state);
      onClose();
    } catch (problem) {
      if (active.current) setError(String(problem));
    } finally {
      if (active.current) {
        setBusy(false);
        setApplying(false);
      }
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !busy && onClose()}>
      <div className="modal translation-dialog" role="dialog" aria-modal="true" aria-label="Translate from another elements.data">
        <header className="modal-head">
          <Languages size={17} /><h3>Translate from elements.data</h3><span className="spacer" />
          <button className="icon-btn" onClick={onClose} disabled={busy} aria-label="Close"><X size={18} /></button>
        </header>
        <div className="translation-intro">
          <p>Copy human-facing UTF-16 names and text from a supported translated file. Lists match by schema identity, records by unique ID, and fields by schema path.</p>
          <p className="muted small">Only text fields are considered. Blank source text, duplicate IDs, incompatible fields and text that does not fit are skipped.</p>
          <div className="translation-file">
            <button className="btn" onClick={choose} disabled={busy} autoFocus><FolderOpen size={14} /> Choose translated file…</button>
            <span className="truncate muted small" title={path}>{path || "A supported elements.data in the language you want"}</span>
            {path && <button className="btn small" onClick={() => preview(path)} disabled={busy}><RefreshCw size={13} /> Refresh preview</button>}
          </div>
        </div>
        {error && <div className="se-problems translation-error" role="alert"><CircleAlert size={14} /> {error}</div>}
        <div className="translation-preview" aria-live="polite">
          {busy ? (
            <p className="muted"><Loader2 size={14} className="spin" /> {applying ? "Applying translations…" : "Matching lists, IDs and text fields…"}</p>
          ) : plan ? (
            <>
              <div className="translation-summary">
                <span><b>{fileName(plan.sourcePath)}</b> v{plan.sourceVersion} → open v{plan.targetVersion}</span>
                <span><b>{count(plan.changedRecords)}</b> records · <b>{count(plan.fieldChanges)}</b> text fields ready</span>
                <span className="muted">{count(plan.matchedRecords)} IDs matched</span>
                {(plan.missingSource > 0 || plan.rejected > 0) && <span className="diff-del">{count(plan.missingSource + plan.rejected)} skipped</span>}
              </div>
              <div className="segmented translation-tabs" role="group" aria-label="Translation preview">
                <button className={tab === "lists" ? "active" : ""} onClick={() => setTab("lists")}>Lists ({plan.lists.length})</button>
                <button className={tab === "changes" ? "active" : ""} onClick={() => setTab("changes")}>Text changes ({count(plan.fieldChanges)})</button>
                <button className={tab === "issues" ? "active" : ""} onClick={() => setTab("issues")}>Issues ({count(plan.issues.length)})</button>
              </div>
              <div className="translation-details scroll">
                {tab === "lists" ? (
                  <table className="translation-table">
                    <thead><tr>
                      <th><input type="checkbox" checked={allSelected} onChange={(event) => setSelected(event.target.checked ? new Set(selectable.map((list) => list.list)) : new Set())} aria-label="Select every list with translations" /></th>
                      <th>Target list</th><th>Source list</th><th>IDs matched</th><th>Records</th><th>Text fields</th><th>Skipped</th>
                    </tr></thead>
                    <tbody>{plan.lists.map((list) => <tr key={list.list} className={selected.has(list.list) ? "picked" : ""}>
                      <td>{list.fieldChanges > 0 && <input type="checkbox" checked={selected.has(list.list)} onChange={(event) => setSelected((current) => {
                        const next = new Set(current); if (event.target.checked) next.add(list.list); else next.delete(list.list); return next;
                      })} />}</td>
                      <td><b>{list.list}</b> · {list.name}</td>
                      <td>{list.sourceList ?? <span className="muted">missing</span>}</td>
                      <td>{count(list.matchedRecords)}</td>
                      <td className={list.changedRecords ? "diff-add" : "muted"}>{count(list.changedRecords)}</td>
                      <td className={list.fieldChanges ? "diff-add" : "muted"}>{count(list.fieldChanges)}</td>
                      <td className={list.missingSource + list.rejected ? "diff-del" : "muted"}>{count(list.missingSource + list.rejected)}</td>
                    </tr>)}</tbody>
                  </table>
                ) : tab === "changes" ? changeRows.length ? (
                  <table className="translation-table changes">
                    <thead><tr><th>List · ID</th><th>Field</th><th>Current text</th><th>Translated text</th></tr></thead>
                    <tbody>{changeRows.map((change, index) => <tr key={`${change.list}:${change.row}:${change.field}:${index}`}>
                      <td>{change.list} · {change.id}</td><td className="mono">{change.field}</td><td>{change.old || <span className="muted">(empty)</span>}</td><td className="diff-add">{change.new}</td>
                    </tr>)}</tbody>
                  </table>
                ) : <p className="muted small">Select at least one list with text changes.</p> : plan.issues.length ? (
                  plan.issues.map((issue, index) => <div className="translation-issue" key={index}>
                    <b>List {issue.list}{issue.id !== undefined ? ` · ID ${issue.id}` : ""}{issue.field ? ` · ${issue.field}` : ""}</b><span>{issue.message}</span>
                  </div>)
                ) : <p className="muted small">No translation issues were found.</p>}
              </div>
              {tab === "changes" && plan.fieldChanges > plan.changes.length && <p className="muted small translation-limit">Showing the first {count(plan.changes.length)} of {count(plan.fieldChanges)} changes.</p>}
            </>
          ) : !error && <p className="muted small">Choose a translated elements.data to preview. Nothing changes until you apply.</p>}
        </div>
        <footer className="modal-foot">
          <span className="muted small">Selected: {count(selectedRecords)} records · {count(selectedFields)} fields · one undo step</span><span className="spacer" />
          <button className="btn" onClick={onClose} disabled={busy}>Cancel</button>
          <button className="btn primary" onClick={apply} disabled={busy || selectedFields === 0}><Check size={14} /> Apply translations</button>
        </footer>
      </div>
    </div>
  );
}
