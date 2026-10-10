import { useEffect, useMemo, useState } from "react";
import { Check, Loader2, Search, X } from "lucide-react";
import { clientImages } from "../elements/api";
import { count } from "../elements/format";

/** The shop pictures' folder in surfaces.pck. */
const FOLDER = "surfaces\\qshop";
const PAGE = 240;

interface Props {
  /** The icon the item has now. */
  current: string;
  /** Icons this shop already uses (lowercase), with how many items use each. */
  used: Map<string, number>;
  iconUrl: (path: string) => string | null;
  onPick: (path: string) => void;
  onClose: () => void;
}

const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;
/** The subfolder below the shop folder ("1", "2", …), or "" for files directly in it. */
const subfolder = (path: string) => path.split(/[\\/]/).slice(2, -1).join("\\");

export function GShopIconPicker({ current, used, iconUrl, onPick, onClose }: Props) {
  const [paths, setPaths] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [folder, setFolder] = useState<string | null>(null);
  const [unused, setUnused] = useState(false);
  const [shown, setShown] = useState(PAGE);
  const [chosen, setChosen] = useState(current);

  useEffect(() => {
    clientImages(FOLDER).then(setPaths).catch((problem) => setError(String(problem).replace(/^Error: /, "")));
  }, []);
  // Start in the current icon's folder.
  useEffect(() => { if (paths && folder === null) setFolder(current ? subfolder(current) : ""); }, [current, folder, paths]);
  useEffect(() => setShown(PAGE), [query, folder, unused]);

  const folders = useMemo(() => [...new Set((paths ?? []).map(subfolder))].sort((a, b) => a.localeCompare(b, undefined, { numeric: true })), [paths]);
  const needle = query.trim().toLowerCase();
  const matches = useMemo(() => (paths ?? []).filter((path) => {
    if (folder && subfolder(path).toLowerCase() !== folder.toLowerCase()) return false;
    if (needle && !fileName(path).toLowerCase().includes(needle)) return false;
    return !unused || !used.has(path.toLowerCase());
  }), [folder, needle, paths, unused, used]);
  const missing = current && paths && !paths.some((path) => path.toLowerCase() === current.toLowerCase());

  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal gshop-icon-picker" role="dialog" aria-label="Choose an icon" onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head"><h3>Choose an icon</h3><span className="muted small">Pictures in the client's surfaces.pck ({FOLDER})</span><span className="spacer" /><button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button></header>
      <div className="npcgen-nearby-bar">
        <div className="dyn-task-search"><Search size={13} /><input autoFocus value={query} placeholder="File name" onChange={(event) => setQuery(event.target.value)} />{query && <button className="icon-btn small" onClick={() => setQuery("")}><X size={12} /></button>}</div>
        {folders.length > 1 && <span className="dyn-compare-tabs">
          <button className={"btn small" + (folder === "" ? " active" : "")} onClick={() => setFolder("")}>All</button>
          {folders.filter(Boolean).map((entry) => <button key={entry} className={"btn small" + (folder?.toLowerCase() === entry.toLowerCase() ? " active" : "")} onClick={() => setFolder(entry)}>{entry}</button>)}
        </span>}
        <label className="npcgen-flag"><input type="checkbox" checked={unused} onChange={(event) => setUnused(event.target.checked)} /> Not used in this shop</label>
        <span className="spacer" />
        {paths && <span className="muted small">{count(matches.length)} of {count(paths.length)}</span>}
      </div>
      <div className="gshop-icon-grid-wrap">
        {error ? <div className="path-data-message error">{error}</div>
          : !paths ? <div className="empty-note"><Loader2 size={14} className="spin" /> Reading the package…</div>
          : !matches.length ? <div className="empty-note">No pictures match.</div>
          : <div className="gshop-icon-grid" role="listbox">
            {matches.slice(0, shown).map((path) => {
              const url = iconUrl(path);
              const uses = used.get(path.toLowerCase()) ?? 0;
              return <button key={path} role="option" aria-selected={chosen.toLowerCase() === path.toLowerCase()} className={"gshop-icon-tile" + (chosen.toLowerCase() === path.toLowerCase() ? " selected" : "")} title={`${path}${uses ? `\nUsed by ${uses} item${uses === 1 ? "" : "s"} here` : ""}`} onClick={() => setChosen(path)} onDoubleClick={() => onPick(path)}>
                {url ? <img src={url} width={48} height={48} alt="" loading="lazy" /> : <span className="gshop-icon empty" style={{ width: 48, height: 48 }} />}
                <span className="small truncate">{fileName(path)}</span>
                {uses > 0 && <span className="gshop-icon-used" />}
              </button>;
            })}
            {matches.length > shown && <button className="btn small gshop-icon-more" onClick={() => setShown((value) => value + PAGE)}>Show {count(Math.min(PAGE, matches.length - shown))} more</button>}
          </div>}
      </div>
      <footer className="modal-foot">
        <span className="muted small truncate">{missing ? `The current icon (${current}) is not in the package.` : <>Double-click to choose. A dot marks icons this shop already uses.</>}</span>
        <span className="spacer" />
        <span className="mono small truncate">{chosen}</span>
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn primary" disabled={!chosen || chosen === current} onClick={() => onPick(chosen)}><Check size={14} /> Choose</button>
      </footer>
    </div>
  </div>;
}
