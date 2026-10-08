import { useDeferredValue, useEffect, useState } from "react";
import { ArrowRight, Loader2, Search, X } from "lucide-react";
import { searchTasks } from "../elements/api";
import type { TaskSearchEntry } from "../elements/types";

interface Props {
  source: TaskSearchEntry;
  busy: boolean;
  error: string | null;
  onConfirm: (destination: TaskSearchEntry) => void;
  onClose: () => void;
}

function isInsideSource(candidate: TaskSearchEntry, source: TaskSearchEntry) {
  return candidate.pack === source.pack
    && candidate.root === source.root
    && candidate.path.length >= source.path.length
    && source.path.every((part, index) => candidate.path[index] === part);
}

export function TaskMoveDialog({ source, busy, error, onConfirm, onClose }: Props) {
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query.trim());
  const [matches, setMatches] = useState<TaskSearchEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [message, setMessage] = useState<string | null>(null);
  const [selected, setSelected] = useState<TaskSearchEntry | null>(null);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || busy) return;
      event.stopImmediatePropagation();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [busy, onClose]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setMessage(null);
    const timer = window.setTimeout(() => {
      void searchTasks(deferredQuery, 100).then((report) => {
        if (cancelled) return;
        if (report.error) {
          setMessage(report.error);
          setMatches([]);
        } else if (!report.indexed) {
          setMessage("Task destinations are still being indexed…");
          setMatches([]);
        } else {
          setMatches(report.matches.filter((candidate) => !isInsideSource(candidate, source)));
        }
      }).catch((problem) => {
        if (!cancelled) {
          setMessage(String(problem).replace(/^Error: /, ""));
          setMatches([]);
        }
      }).finally(() => {
        if (!cancelled) setLoading(false);
      });
    }, deferredQuery ? 150 : 0);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [deferredQuery, source]);

  return <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !busy && onClose()}>
    <div className="modal task-move-dialog" role="dialog" aria-labelledby="task-move-title">
      <header className="modal-head"><ArrowRight size={18}/><div><h3 id="task-move-title">Move subquest subtree</h3><p className="muted small">Choose the quest that will become the new parent.</p></div><span className="spacer"/><button className="icon-btn" onClick={onClose} disabled={busy} aria-label="Close"><X size={18}/></button></header>
      <div className="task-move-source"><span>Moving</span><b>{source.id} · {source.name || "(unnamed task)"}</b><span>It will be appended as the new parent’s last subquest. Task IDs and references remain unchanged.</span></div>
      <div className="task-move-search"><Search size={15}/><input value={query} onChange={(event) => { setQuery(event.target.value); setSelected(null); }} placeholder="Find a destination quest by ID or name…" autoFocus autoComplete="off"/></div>
      <div className="task-move-results">
        {loading ? <div className="empty-note center"><Loader2 size={16} className="spin"/> Finding destinations…</div>
          : message ? <div className="empty-note center">{message}</div>
            : matches.length ? matches.map((candidate) => {
              const active = selected?.pack === candidate.pack && selected.root === candidate.root && selected.path.join(".") === candidate.path.join(".");
              return <button className={"task-move-result" + (active ? " selected" : "")} key={`${candidate.pack}:${candidate.root}:${candidate.path.join(".")}`} onClick={() => setSelected(candidate)}>
                <span className="mono">{candidate.id}</span><span className="truncate">{candidate.name || "(unnamed task)"}</span><span className="muted">pack {candidate.pack + 1} · root {candidate.root + 1}{candidate.path.length ? ` · subquest ${candidate.path.map((part) => part + 1).join(".")}` : ""}</span>
              </button>;
            }) : <div className="empty-note center">No eligible destination quest matches this search.</div>}
      </div>
      {error && <div className="path-data-message error" role="alert">{error}</div>}
      <footer className="modal-foot"><span className="muted small">Moving across roots changes both roots as one undoable operation.</span><span className="spacer"/><button className="btn" onClick={onClose} disabled={busy}>Cancel</button><button className="btn primary" onClick={() => selected && onConfirm(selected)} disabled={!selected || busy}>{busy ? <Loader2 size={14} className="spin"/> : <ArrowRight size={14}/>} {busy ? "Moving…" : "Move subtree"}</button></footer>
    </div>
  </div>;
}
