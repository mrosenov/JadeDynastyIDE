import { useMemo, useState } from "react";
import { Check, FolderOpen, X } from "lucide-react";
import { count } from "../elements/format";
import type { PathDataRow } from "../elements/types";

type Status = "changed" | "there" | "here" | "same";
const LABELS: Record<Status, string> = { changed: "Different file", there: "Only there", here: "Only here", same: "Same" };

interface Pair {
  id: number;
  here: string | null;
  there: string | null;
  status: Status;
}

interface Props {
  /** The other table (a DynamicObjects.data or a JSON export) and where it came from. */
  other: { path: string; rows: PathDataRow[] };
  rows: PathDataRow[];
  onCopy: (copy: PathDataRow[]) => void;
  onChooseOther: () => void;
  onClose: () => void;
}

/** The open table against another client's: objects by ID; copying sets their files here (one undo step). */
export function DynObjectsCompare({ other, rows, onCopy, onChooseOther, onClose }: Props) {
  const pairs = useMemo(() => {
    const here = new Map(rows.map((row) => [row.id, row.path]));
    const there = new Map(other.rows.map((row) => [row.id, row.path]));
    const ids = [...new Set([...here.keys(), ...there.keys()])].sort((a, b) => a - b);
    return ids.map((id): Pair => {
      const a = here.get(id) ?? null, b = there.get(id) ?? null;
      return { id, here: a, there: b, status: a === null ? "there" : b === null ? "here" : a === b ? "same" : "changed" };
    });
  }, [other, rows]);
  const counts = useMemo(() => {
    const out: Record<Status, number> = { changed: 0, there: 0, here: 0, same: 0 };
    for (const pair of pairs) out[pair.status]++;
    return out;
  }, [pairs]);
  const [filter, setFilter] = useState<Status | "differ">("differ");
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const shown = pairs.filter((pair) => filter === "differ" ? pair.status !== "same" : pair.status === filter);
  const copyable = (pair: Pair) => pair.status === "changed" || pair.status === "there";
  // A file another ID here already uses would repeat a path: the client would drop the whole table.
  const clashes = useMemo(() => {
    const out = new Map<number, number>();
    const byPath = new Map(rows.map((row) => [row.path, row.id]));
    for (const pair of pairs) {
      if (!copyable(pair) || pair.there === null) continue;
      const owner = byPath.get(pair.there);
      if (owner !== undefined && owner !== pair.id && !picked.has(owner)) out.set(pair.id, owner);
    }
    return out;
  }, [pairs, picked, rows]);
  const chosen = pairs.filter((pair) => picked.has(pair.id) && copyable(pair));
  const blocked = chosen.filter((pair) => clashes.has(pair.id));

  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal npcgen-nearby-dialog gshop-text-update" role="dialog" aria-label="Compare" onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head"><h3>Compare with <span className="mono" title={other.path}>{other.path.split(/[\\/]/).pop()}</span></h3><span className="spacer" /><button className="btn small" onClick={onChooseOther}><FolderOpen size={13} /> Other file…</button><button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button></header>
      <div className="npcgen-nearby-bar">
        <span className="dyn-compare-tabs">
          <button className={"btn small" + (filter === "differ" ? " active" : "")} onClick={() => setFilter("differ")}>Differ <span className="muted">{count(counts.changed + counts.there + counts.here)}</span></button>
          {(Object.keys(LABELS) as Status[]).map((status) => <button key={status} className={"btn small" + (filter === status ? " active" : "")} onClick={() => setFilter(status)}>{LABELS[status]} <span className="muted">{count(counts[status])}</span></button>)}
        </span>
      </div>
      <div className="npcgen-nearby-table">
        {!shown.length ? <div className="empty-note">No object.</div> : <table className="dyn-table gshop-text-table">
          <thead><tr>
            <th><input type="checkbox" title="Pick every object shown that can be copied" checked={shown.some(copyable) && shown.filter(copyable).every((pair) => picked.has(pair.id))} onChange={(event) => setPicked((current) => { const next = new Set(current); for (const pair of shown.filter(copyable)) { if (event.target.checked) next.add(pair.id); else next.delete(pair.id); } return next; })} /></th>
            <th>ID</th><th>Here</th><th>There</th>
          </tr></thead>
          <tbody>{shown.map((pair) => <tr key={pair.id} className={picked.has(pair.id) ? "selected" : undefined}>
            <td>{copyable(pair) ? <input type="checkbox" checked={picked.has(pair.id)} onChange={() => setPicked((current) => { const next = new Set(current); if (next.has(pair.id)) next.delete(pair.id); else next.add(pair.id); return next; })} /> : null}</td>
            <td className="mono">{pair.id}</td>
            <td className="gshop-text-cell mono muted">{pair.here ?? <i>—</i>}</td>
            <td className="gshop-text-cell mono">{pair.there ?? <i>—</i>}{clashes.has(pair.id) && <div className="ai-warn small">Object {clashes.get(pair.id)} here already uses this file: copy that one too, or change it first.</div>}</td>
          </tr>)}</tbody>
        </table>}
      </div>
      <footer className="npcgen-nearby-foot">
        <span className="muted small">Copying sets the file of an ID here (or adds the ID). Objects only here stay. One undo step.</span>
        <span className="spacer" />
        <button className="btn" onClick={onClose}>Close</button>
        <button className="btn primary" disabled={!chosen.length || blocked.length > 0} title={blocked.length ? "Some picked files are used by other objects here" : undefined} onClick={() => { onCopy(chosen.map((pair) => ({ id: pair.id, path: pair.there! }))); setPicked(new Set()); }}><Check size={14} /> Copy {count(chosen.length)} object{chosen.length === 1 ? "" : "s"}</button>
      </footer>
    </div>
  </div>;
}
