import { useEffect, useRef, useState } from "react";
import { Box, ChevronLeft, ChevronRight, Loader2, Search, Sparkles, X } from "lucide-react";
import { clientDynFiles } from "../elements/api";
import { count } from "../elements/format";
import type { PathDataRow } from "../elements/types";

const PAGE = 200;

interface Props {
  /** The kind to show first. */
  kind: "model" | "effect";
  current?: string;
  onPick: (path: string) => void;
  onClose: () => void;
}

/** Models (.ecm in models.pck) and effects (.gfx in gfx.pck) of the configured client. */
export function DynFilePicker({ kind: initialKind, current, onPick, onClose }: Props) {
  const [kind, setKind] = useState(initialKind);
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(0);
  const [result, setResult] = useState<{ files: string[]; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => input.current?.focus(), []);
  useEffect(() => {
    let cancelled = false;
    setResult(null);
    setError(null);
    const timer = window.setTimeout(() => {
      clientDynFiles(kind, query, page * PAGE, PAGE)
        .then((next) => { if (!cancelled) setResult(next); })
        .catch((problem) => { if (!cancelled) setError(String(problem).replace(/^Error: /, "")); });
    }, 200);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [kind, page, query]);

  const pages = Math.max(1, Math.ceil((result?.total ?? 0) / PAGE));
  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal npcgen-nearby-dialog dyn-file-picker" role="dialog" aria-label="Choose a model or effect" onMouseDown={(event) => event.stopPropagation()} onKeyDown={(event) => { if (event.key === "Escape") onClose(); }}>
      <header className="modal-head"><h3>Choose a {kind === "model" ? "model" : "effect"}</h3><span className="muted small">from the configured client</span><span className="spacer" /><button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button></header>
      <div className="npcgen-nearby-bar">
        <span className="dyn-compare-tabs">
          <button className={"btn small" + (kind === "model" ? " active" : "")} onClick={() => { setKind("model"); setPage(0); }}><Box size={13} /> Models (.ecm)</button>
          <button className={"btn small" + (kind === "effect" ? " active" : "")} onClick={() => { setKind("effect"); setPage(0); }}><Sparkles size={13} /> Effects (.gfx)</button>
        </span>
        <span className="dyn-task-search ai-cmp-search dyn-file-search"><Search size={13} /><input ref={input} value={query} placeholder="Words of the path" onChange={(event) => { setQuery(event.target.value); setPage(0); }} /></span>
        <span className="spacer" />
        {result && <span className="muted small">{count(result.total)} file{result.total === 1 ? "" : "s"}</span>}
      </div>
      {error && <div className="path-data-message error">{error}</div>}
      <div className="npcgen-nearby-table dyn-file-list">
        {!result ? !error && <div className="empty-note"><Loader2 size={14} className="spin" /> Reading the client's packages…</div>
          : !result.files.length ? <div className="empty-note">No file matches.</div>
          : result.files.map((path) => <button key={path} className={"dyn-file-row mono" + (current && path.toLowerCase() === current.toLowerCase() ? " selected" : "")} onClick={() => onPick(path)} title={path}>{path}</button>)}
      </div>
      <footer className="npcgen-nearby-foot">
        <span className="muted small">{kind === "model" ? "Models are stored from the client root (models\\…)." : "Effects are stored below gfx\\ (the client adds it)."}</span>
        <span className="spacer" />
        {pages > 1 && <>
          <button className="icon-btn small" disabled={page === 0} onClick={() => setPage(page - 1)} aria-label="Previous page"><ChevronLeft size={14} /></button>
          <span className="muted small">Page {page + 1} of {count(pages)}</span>
          <button className="icon-btn small" disabled={page + 1 >= pages} onClick={() => setPage(page + 1)} aria-label="Next page"><ChevronRight size={14} /></button>
        </>}
        <button className="btn" onClick={onClose}>Cancel</button>
      </footer>
    </div>
  </div>;
}

/** The dynamic objects of a DynamicObjects.data table, to choose one for npcgen.data. */
export function DynObjectPicker({ objects, current, onPick, onClose }: { objects: PathDataRow[]; current: number; onPick: (id: number) => void; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const shown = objects.filter((row) => words.every((word) => String(row.id) === word || row.path.toLowerCase().includes(word)));
  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal npcgen-nearby-dialog dyn-file-picker" role="dialog" aria-label="Choose a dynamic object" onMouseDown={(event) => event.stopPropagation()} onKeyDown={(event) => { if (event.key === "Escape") onClose(); }}>
      <header className="modal-head"><h3>Choose a dynamic object</h3><span className="muted small">from the configured client's DynamicObjects.data</span><span className="spacer" /><button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button></header>
      <div className="npcgen-nearby-bar">
        <span className="dyn-task-search ai-cmp-search dyn-file-search"><Search size={13} /><input ref={input} value={query} placeholder="ID or words of the file" onChange={(event) => setQuery(event.target.value)} /></span>
        <span className="spacer" />
        <span className="muted small">{count(shown.length)} of {count(objects.length)}</span>
      </div>
      <div className="npcgen-nearby-table dyn-file-list">
        {!shown.length ? <div className="empty-note">No object matches.</div> : shown.map((row) => <button key={row.id} className={"dyn-file-row" + (row.id === current ? " selected" : "")} onClick={() => onPick(row.id)} title={row.path}>
          <span className="mono">{row.id}</span> {row.path.toLowerCase().endsWith(".gfx") ? <Sparkles size={11} /> : <Box size={11} />} <span className="mono">{row.path || "(no file)"}</span>
        </button>)}
      </div>
      <footer className="npcgen-nearby-foot"><span className="muted small">Edit the table in the Objects workspace (DynamicObjects.data); this list is the saved file.</span><span className="spacer" /><button className="btn" onClick={onClose}>Cancel</button></footer>
    </div>
  </div>;
}
