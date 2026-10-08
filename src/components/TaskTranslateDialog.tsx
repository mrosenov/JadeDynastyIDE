import { useEffect, useMemo, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Check, CircleAlert, FolderOpen, Languages, Loader2, RefreshCw, X } from "lucide-react";
import { applyTaskTranslation, closeTaskTranslation, previewTaskTranslation } from "../elements/api";
import { count } from "../elements/format";
import type { TaskEditState, TaskTextGroup, TaskTranslationReport } from "../elements/types";

interface Props {
  /** Folder to start the file picker in. */
  defaultPath?: string;
  onApplied: (state: TaskEditState, fields: number) => void;
  onClose: () => void;
}

const GROUP_LABELS: Record<TaskTextGroup, { title: string; detail: string }> = {
  names: { title: "Names", detail: "Task names and signatures" },
  descriptions: { title: "Descriptions", detail: "Description, success, failure, tribute, hint and can-deliver texts" },
  dialogs: { title: "Dialogs", detail: "NPC window texts, player options and talk prompts" },
};
const shorten = (text: string) => text.replace(/\0+$/, "").replace(/\r\n/g, " ↵ ");

/** Copies names, descriptions and dialog text from a translated tasks.data by task ID. */
export function TaskTranslateDialog({ defaultPath, onApplied, onClose }: Props) {
  const [path, setPath] = useState("");
  const [plan, setPlan] = useState<TaskTranslationReport | null>(null);
  const [selected, setSelected] = useState<Set<TaskTextGroup>>(new Set());
  const [tab, setTab] = useState<"groups" | "changes" | "issues">("groups");
  const [busy, setBusy] = useState(false);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const active = useRef(true);

  useEffect(() => {
    active.current = true;
    return () => {
      active.current = false;
      closeTaskTranslation().catch(() => {});
    };
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
      const result = await previewTaskTranslation(sourcePath);
      if (!active.current) return;
      setPlan(result);
      setSelected(new Set(result.groups.filter((group) => group.fields > 0).map((group) => group.group)));
      setTab("groups");
    } catch (problem) {
      if (active.current) setError(String(problem).replace(/^Error: /, ""));
    } finally {
      if (active.current) setBusy(false);
    }
  };

  const choose = async () => {
    setError(null);
    const picked = await open({ multiple: false, directory: false, defaultPath, title: "Choose the translated tasks.data", filters: [{ name: "tasks.data", extensions: ["data"] }] });
    if (typeof picked === "string" && active.current) {
      setPath(picked);
      await preview(picked);
    }
  };

  const chosen = useMemo(() => plan?.groups.filter((group) => selected.has(group.group)) ?? [], [plan, selected]);
  const selectedFields = chosen.reduce((sum, group) => sum + group.fields, 0);
  const selectable = plan?.groups.filter((group) => group.fields > 0) ?? [];
  const allSelected = selectable.length > 0 && selectable.every((group) => selected.has(group.group));
  const samples = plan?.samples.filter((sample) => selected.has(sample.group)) ?? [];

  const apply = async () => {
    if (!plan || !selectedFields || busy) return;
    setBusy(true);
    setApplying(true);
    setError(null);
    try {
      const state = await applyTaskTranslation(plan.token, [...selected]);
      onApplied(state, selectedFields);
      onClose();
    } catch (problem) {
      if (active.current) setError(String(problem).replace(/^Error: /, ""));
    } finally {
      if (active.current) {
        setBusy(false);
        setApplying(false);
      }
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !busy && onClose()}>
      <div className="modal translation-dialog" role="dialog" aria-modal="true" aria-label="Translate from another tasks.data">
        <header className="modal-head">
          <Languages size={17} /><h3>Translate from tasks.data</h3><span className="spacer" />
          <button className="icon-btn" onClick={onClose} disabled={busy} aria-label="Close"><X size={18} /></button>
        </header>
        <div className="translation-intro">
          <p>Copy task names, descriptions and NPC dialog text from a translated task set into the open one. Tasks match by <b>task ID</b> and texts by field; the versions may differ.</p>
          <p className="muted small">Only text changes; IDs, numbers and counts never do. A talk is translated only when its windows and options have the same shape on both sides. Blank source text and text too long for its field are skipped.</p>
          <div className="translation-file">
            <button className="btn" onClick={() => void choose()} disabled={busy} autoFocus><FolderOpen size={14} /> Choose translated tasks.data…</button>
            <span className="truncate muted small" title={path}>{path || "A supported tasks.data in the language you want"}</span>
            {path && <button className="btn small" onClick={() => void preview(path)} disabled={busy}><RefreshCw size={13} /> Refresh preview</button>}
          </div>
        </div>
        {error && <div className="se-problems translation-error" role="alert"><CircleAlert size={14} /> {error}</div>}
        <div className="translation-preview" aria-live="polite">
          {busy ? (
            <p className="muted"><Loader2 size={14} className="spin" /> {applying ? "Applying translations…" : "Matching tasks and texts… Two different versions can take half a minute."}</p>
          ) : plan ? (
            <>
              <div className="translation-summary">
                <span>tasks.data v{plan.sourceVersion} → v{plan.version}</span>
                <span className="muted">{count(plan.matched)} task IDs matched</span>
                {plan.missingSource > 0 && <span className="muted">{count(plan.missingSource)} not in the source</span>}
                {plan.blank > 0 && <span className="muted" title="Open texts kept because the source text is blank">{count(plan.blank)} blank in the source</span>}
                {plan.shape > 0 && <span className="diff-del" title="Talks whose windows or options differ in shape are left as they are">{count(plan.shape)} talk{plan.shape === 1 ? "" : "s"} shaped differently</span>}
                {plan.tooLong > 0 && <span className="diff-del">{count(plan.tooLong)} too long</span>}
                {plan.ambiguous > 0 && <span className="muted">{count(plan.ambiguous)} shared IDs skipped</span>}
              </div>
              <div className="segmented translation-tabs" role="group" aria-label="Translation preview">
                <button className={tab === "groups" ? "active" : ""} onClick={() => setTab("groups")}>Text groups</button>
                <button className={tab === "changes" ? "active" : ""} onClick={() => setTab("changes")}>Samples ({count(samples.length)})</button>
                <button className={tab === "issues" ? "active" : ""} onClick={() => setTab("issues")}>Issues ({count(plan.issues.length)})</button>
              </div>
              <div className="translation-details scroll">
                {tab === "groups" ? (
                  <table className="translation-table">
                    <thead><tr>
                      <th><input type="checkbox" checked={allSelected} onChange={(event) => setSelected(event.target.checked ? new Set(selectable.map((group) => group.group)) : new Set())} aria-label="Select every group with translations" /></th>
                      <th>Texts</th><th>Tasks</th><th>Text fields</th><th>Too long</th>
                    </tr></thead>
                    <tbody>{plan.groups.map((group) => <tr key={group.group} className={selected.has(group.group) ? "picked" : ""}>
                      <td>{group.fields > 0 && <input type="checkbox" checked={selected.has(group.group)} onChange={(event) => setSelected((current) => {
                        const next = new Set(current); if (event.target.checked) next.add(group.group); else next.delete(group.group); return next;
                      })} />}</td>
                      <td><b>{GROUP_LABELS[group.group].title}</b><div className="muted small">{GROUP_LABELS[group.group].detail}</div></td>
                      <td className={group.tasks ? "diff-add" : "muted"}>{count(group.tasks)}</td>
                      <td className={group.fields ? "diff-add" : "muted"}>{count(group.fields)}</td>
                      <td className={group.tooLong ? "diff-del" : "muted"}>{count(group.tooLong)}</td>
                    </tr>)}</tbody>
                  </table>
                ) : tab === "changes" ? samples.length ? (
                  <table className="translation-table changes">
                    <thead><tr><th>Task</th><th>Field</th><th>Current text</th><th>Translated text</th></tr></thead>
                    <tbody>{samples.map((sample, index) => <tr key={`${sample.id}:${sample.field}:${index}`}>
                      <td title={sample.name}>{sample.id}</td><td className="mono">{sample.field}</td><td>{shorten(sample.old) || <span className="muted">(empty)</span>}</td><td className="diff-add">{shorten(sample.new)}</td>
                    </tr>)}</tbody>
                  </table>
                ) : <p className="muted small">Select at least one group with text changes.</p> : plan.issues.length ? (
                  plan.issues.map((issue, index) => <div className="translation-issue" key={index}>
                    <b>Task {issue.id}{issue.name ? ` · ${issue.name}` : ""}{issue.field ? ` · ${issue.field}` : ""}</b><span>{issue.message}</span>
                  </div>)
                ) : <p className="muted small">No translation issues were found.</p>}
              </div>
              {tab === "changes" && <p className="muted small translation-limit">Up to 100 samples per group; every listed change is applied.</p>}
            </>
          ) : !error && <p className="muted small">Choose a translated tasks.data to preview. Nothing changes until you apply.</p>}
        </div>
        <footer className="modal-foot">
          <span className="muted small">Selected: {count(selectedFields)} text{selectedFields === 1 ? "" : "s"} · one undo step · save afterwards to write tasks.data</span><span className="spacer" />
          <button className="btn" onClick={onClose} disabled={busy}>Cancel</button>
          <button className="btn primary" onClick={() => void apply()} disabled={busy || selectedFields === 0}><Check size={14} /> Apply translations</button>
        </footer>
      </div>
    </div>
  );
}
