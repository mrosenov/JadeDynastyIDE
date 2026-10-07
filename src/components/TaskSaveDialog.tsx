import { useCallback, useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Archive, FileWarning, FolderOpen, Loader2, Save, ShieldCheck, TriangleAlert, X } from "lucide-react";
import { saveTasks, taskSavePlan } from "../elements/api";
import type { TaskSaveOptions, TaskSavePlan, TaskSaveReport } from "../elements/types";
import { bytes } from "../elements/format";

interface Props {
  path: string;
  onCancel: () => void;
  onSaved: (report: TaskSaveReport) => void;
}

const BACKUP_KEY = "jdide.tasks.save.backup";
const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

function readBackup() {
  try { return localStorage.getItem(BACKUP_KEY) !== "0"; }
  catch { return true; }
}

function writeBackup(value: boolean) {
  try { localStorage.setItem(BACKUP_KEY, value ? "1" : "0"); }
  catch { /* Remembering the choice is optional. */ }
}

export function TaskSaveDialog({ path: initialPath, onCancel, onSaved }: Props) {
  const [path, setPath] = useState(initialPath);
  const [backup, setBackup] = useState(readBackup);
  const [plan, setPlan] = useState<TaskSavePlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setPlan(null);
    setError(null);
    taskSavePlan({ path, backup }).then(setPlan).catch((problem) => setError(String(problem).replace(/^Error: /, "")));
  }, [backup, path]);

  const run = useCallback(async () => {
    if (!plan || saving) return;
    setSaving(true);
    setError(null);
    const options: TaskSaveOptions = { path, backup };
    try {
      onSaved(await saveTasks(options));
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
      setSaving(false);
    }
  }, [backup, onSaved, path, plan, saving]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !saving) onCancel();
      else if (event.key === "Enter" && !(event.target instanceof HTMLButtonElement)) {
        event.preventDefault();
        void run();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel, run, saving]);

  const pick = async () => {
    const chosen = await save({ defaultPath: path, title: "Save tasks.data", filters: [{ name: "tasks.data", extensions: ["data"] }] });
    if (chosen) setPath(chosen);
  };

  return <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !saving && onCancel()}>
    <div className="modal save-dialog" role="dialog" aria-label="Save tasks.data">
      <header className="modal-head"><Save size={16} className="accent-icon" /><h3>Save {fileName(path)}</h3><span className="spacer"/><button className="icon-btn" onClick={onCancel} disabled={saving} aria-label="Close"><X size={18}/></button></header>
      <div className="save-body">
        <div className="save-target"><div className="save-target-path mono truncate" title={path}>{path}</div><button className="btn small" onClick={pick} disabled={saving}><FolderOpen size={13}/> Save as…</button></div>
        {!plan && !error && <div className="muted small"><Loader2 size={13} className="spin"/> Inspecting the task set…</div>}
        {plan && <>
          <div className="save-facts"><span className={"tag" + (plan.replaces ? " warn" : "")}>{plan.replaces ? (plan.sameFile ? "Replaces the open task set" : "Replaces an existing task set") : "New task set"}</span><span className="muted small">{bytes(plan.size)}</span><span className="spacer"/><span className="small">{plan.changedRoots} changed roots · {plan.changedPacks} changed packs · {plan.packCount} packs total</span></div>
          {plan.changedOnDisk && <div className="save-note danger"><FileWarning size={15}/><div><b>The task set changed on disk.</b> Reopen it before saving so unchanged packs cannot be mixed with outside changes.</div></div>}
          {plan.readOnly && <div className="save-note warn"><TriangleAlert size={15}/><div>One or more destination files are read-only. Saving clears that flag.</div></div>}
          <div className="save-note ok"><ShieldCheck size={15}/><div>JD IDE stages the complete index and pack set, verifies every MD5, parses every root, and checks an exact byte round trip before replacing anything.</div></div>
          <label className={"save-backup" + (plan.replaces ? "" : " off")}><input type="checkbox" checked={backup} disabled={saving || !plan.replaces} onChange={(event) => { setBackup(event.target.checked); writeBackup(event.target.checked); }}/><Archive size={14}/><span>Keep a backup of the complete replaced task set<span className="muted small save-backup-name">{plan.backup ? ` · ${fileName(plan.backup)}` : ""}</span></span></label>
        </>}
        {error && <div className="se-problems">{error}</div>}
      </div>
      <footer className="modal-foot"><span className="muted small">Undo remains available after saving.</span><span className="spacer"/><button className="btn" onClick={onCancel} disabled={saving}>Cancel</button><button className="btn primary" onClick={() => void run()} disabled={!plan || plan.changedOnDisk || saving}>{saving ? <Loader2 size={14} className="spin"/> : <Save size={14}/>} {saving ? "Validating and saving…" : "Save"}</button></footer>
    </div>
  </div>;
}
