import { useEffect, useState } from "react";
import { AlertTriangle, Check, Hash, Loader2, Search, X } from "lucide-react";
import { changeTaskId, previewTaskIdChange } from "../elements/api";
import type { TaskIdChangePreview, TaskIdChangeResult } from "../elements/types";
import { describeTaskField } from "./TaskForm";

interface Props {
  task: { pack: number; root: number; path: number[]; id: number; name: string };
  onApplied: (result: TaskIdChangeResult, newId: number) => void;
  onClose: () => void;
}

const fieldName = (field: string) => {
  const place = describeTaskField(field.replace(/\[\d+\]/g, ""));
  return `${place.tabLabel} › ${place.group} › ${place.label}`;
};

/** Gives a quest a new ID: preview what changes in tasks.data and elements.data, then apply. */
export function TaskIdDialog({ task, onApplied, onClose }: Props) {
  const [value, setValue] = useState("");
  const [preview, setPreview] = useState<TaskIdChangePreview | null>(null);
  const [ticked, setTicked] = useState<Set<number>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const newId = Number(value.trim());
  const valid = /^\d+$/.test(value.trim()) && newId > 0 && newId <= 0xffffffff;
  const current = preview && preview.newId === newId;

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || busy) return;
      event.stopImmediatePropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [busy, onClose]);

  const check = async () => {
    if (!valid) return;
    setBusy(true);
    setError(null);
    try {
      const next = await previewTaskIdChange(task.pack, task.root, task.path, task.id, newId);
      setPreview(next);
      // With another quest on the same ID, elements.data places may mean that quest: left unticked.
      setTicked(new Set(next.duplicates ? [] : next.elementUses.map((_, index) => index)));
    } catch (problem) {
      setPreview(null);
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  };
  const apply = async () => {
    if (!preview || !current) return;
    setBusy(true);
    setError(null);
    try {
      const places = preview.elementUses.filter((_, index) => ticked.has(index)).map((use) => [use.list, use.row, use.off] as [number, number, number]);
      onApplied(await changeTaskId(task.pack, task.root, task.path, task.id, newId, places), newId);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
      setBusy(false);
    }
  };

  return <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !busy && onClose()}>
    <div className="modal task-id-dialog" role="dialog" aria-labelledby="task-id-title">
      <header className="modal-head"><Hash size={18} /><div><h3 id="task-id-title">Change quest ID</h3><p className="muted small">{task.name || "(unnamed quest)"} · now {task.id}</p></div><span className="spacer" /><button className="icon-btn" onClick={onClose} disabled={busy} aria-label="Close"><X size={18} /></button></header>
      <div className="task-id-body">
        <label className="task-id-input">
          <span className="task-form-label">New ID</span>
          <input className="task-form-input mono" autoFocus value={value} onChange={(event) => setValue(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") void (current ? apply() : check()); }} placeholder="e.g. 40001" />
          <button className="btn" onClick={() => void check()} disabled={!valid || busy}><Search size={14} /> Preview</button>
        </label>
        <div className="task-delete-warning task-id-note"><header><AlertTriangle size={16} /><div><b>Players keep quest progress by ID.</b><span>On a live server, players who have this quest active or finished lose that record, and other files that name the quest (task_npc.data, server scripts) keep the old ID.</span></div></header></div>
        {preview && current && <>
          <div className="task-delete-summary">
            <div><span>Quest</span><b>{preview.oldId} → {preview.newId}</b></div>
            <div><span>tasks.data</span><b>{preview.references.length} reference{preview.references.length === 1 ? "" : "s"} in {preview.roots} top-level quest{preview.roots === 1 ? "" : "s"}</b></div>
          </div>
          {preview.duplicates > 0 && <div className="task-delete-warning"><header><AlertTriangle size={16} /><div><b>{preview.duplicates} other quest{preview.duplicates === 1 ? "" : "s"} also use{preview.duplicates === 1 ? "s" : ""} ID {preview.oldId}.</b><span>Only references inside this quest's own tree are changed; other references may mean the other quest. elements.data places start unticked.</span></div></header></div>}
          {preview.references.length > 0 && <section className="task-id-section">
            <div className="task-form-subtitle">Quest references rewritten</div>
            <div className="task-id-list">{preview.references.map((reference, index) => <div key={index}><span><b>{reference.taskId}</b> · {reference.taskName || "(unnamed)"}</span><span className="muted truncate" title={reference.field}>{fieldName(reference.field)}</span></div>)}</div>
          </section>}
          <section className="task-id-section">
            <div className="task-form-subtitle">elements.data</div>
            {!preview.elementsPath ? <div className="muted small">No elements.data is open, so NPC quest lists and other places there are not updated. Open the matching elements.data first to update them too.</div>
              : !preview.elementUses.length ? <div className="task-delete-safe"><Check size={15} /><span>No quest-ID field of {preview.elementsPath.split(/[\\/]/).pop()} holds {preview.oldId}.</span></div>
              : <>
                <label className="check task-id-all"><input type="checkbox" checked={ticked.size === preview.elementUses.length} ref={(input) => { if (input) input.indeterminate = ticked.size > 0 && ticked.size < preview.elementUses.length; }} onChange={(event) => setTicked(new Set(event.target.checked ? preview.elementUses.map((_, index) => index) : []))} /> Update {ticked.size} of {preview.elementUses.length} place{preview.elementUses.length === 1 ? "" : "s"} (one undo step in elements.data; save it there)</label>
                <div className="task-id-list">{preview.elementUses.map((use, index) => <label key={index} className="task-id-use">
                  <input type="checkbox" checked={ticked.has(index)} onChange={(event) => setTicked((current) => { const next = new Set(current); if (event.target.checked) next.add(index); else next.delete(index); return next; })} />
                  <span><b>{use.id}</b> · {use.name || "(unnamed)"}</span><span className="muted truncate">{use.listName} › <span className="mono">{use.field}</span></span>
                </label>)}</div>
              </>}
          </section>
        </>}
        {error && <div className="path-data-message error" role="alert">{error}</div>}
      </div>
      <footer className="modal-foot"><span className="muted small">One undo step in tasks.data.</span><span className="spacer" /><button className="btn" onClick={onClose} disabled={busy}>Cancel</button><button className="btn primary" onClick={() => void (current ? apply() : check())} disabled={!valid || busy}>{busy ? <Loader2 size={14} className="spin" /> : current ? <Check size={14} /> : <Search size={14} />} {current ? `Change to ${newId}` : "Preview"}</button></footer>
    </div>
  </div>;
}
