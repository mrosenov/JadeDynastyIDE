import { useMemo, useState } from "react";
import { Check, FolderOpen, X } from "lucide-react";
import { count } from "../elements/format";
import type { DropFile, DropTable } from "../elements/types";

type Status = "changed" | "there" | "here" | "same";
const LABELS: Record<Status, string> = { changed: "Changed", there: "Only there", here: "Only here", same: "Same" };

interface Pair {
  name: string;
  here: number | null;
  there: number | null;
  status: Status;
  /** What differs, in words. */
  parts: string[];
}

interface Props {
  other: { path: string; file: DropFile };
  file: DropFile;
  /** Names of monsters and items (elements.data). */
  nameOf: (id: number) => string | undefined;
  /** Copies tables of the other file: replacing the one of the same name here, or added at the end. */
  onCopy: (tables: DropTable[]) => void;
  onChooseOther: () => void;
  onClose: () => void;
}

const used = (table: DropTable) => { const end = table.items.findIndex((item) => item.id === 0); return end < 0 ? table.items : table.items.slice(0, end); };
const percent = (value: number) => `${Math.round(value * 1_000_000) / 10_000}%`;

/** What differs; the extra value only when both files have it (version 2). */
function differences(a: DropTable, b: DropTable, extra: boolean): string[] {
  const parts: string[] = [];
  if (a.type !== b.type) parts.push("type");
  if (a.counts.some((value, index) => value !== b.counts[index])) parts.push("drop counts");
  const monsters = (table: DropTable) => [...table.monsters].sort((x, y) => x - y).join(",");
  if (monsters(a) !== monsters(b)) parts.push("monsters");
  const items = (table: DropTable) => used(table).map((item) => `${item.id}:${item.chance}`).join(",");
  if (items(a) !== items(b)) parts.push("items");
  if (extra && a.extra !== b.extra) parts.push("extra value");
  return parts;
}

/** Pairs tables by name (repeated names in order). */
function pair(here: DropFile, there: DropFile): Pair[] {
  const queues = new Map<string, number[]>();
  there.tables.forEach((table, index) => queues.set(table.name, [...(queues.get(table.name) ?? []), index]));
  const out: Pair[] = here.tables.map((table, index) => {
    const other = queues.get(table.name)?.shift() ?? null;
    if (other === null) return { name: table.name, here: index, there: null, status: "here", parts: [] };
    const parts = differences(table, there.tables[other], here.version === 2 && there.version === 2);
    return { name: table.name, here: index, there: other, status: parts.length ? "changed" : "same", parts };
  });
  for (const rest of queues.values()) for (const index of rest) out.push({ name: there.tables[index].name, here: null, there: index, status: "there", parts: [] });
  return out;
}

function TableSide({ table, nameOf }: { table: DropTable | null; nameOf: (id: number) => string | undefined }) {
  if (!table) return <div className="ai-cmp-side empty muted small">—</div>;
  return <div className="ai-cmp-side">
    <div><b>{table.type === 1 ? "Replace" : table.type === 2 ? "Add" : `type ${table.type}`}</b> · counts {table.counts.map((value, index) => value ? `${index}: ${percent(value)}` : null).filter(Boolean).join(", ")}</div>
    <div className="small"><span className="muted">Monsters</span> {table.monsters.map((id) => `${nameOf(id) ?? id}`).join(", ") || "none"}</div>
    <ol className="ai-cmp-ops small">{used(table).map((item, index) => <li key={index}>{nameOf(item.id) ?? item.id} <span className="muted">{percent(item.chance)}</span></li>)}</ol>
  </div>;
}

/** The open extra_drops.sev against another one: tables by name, copying whole tables. */
export function DropsCompare({ other, file, nameOf, onCopy, onChooseOther, onClose }: Props) {
  const pairs = useMemo(() => pair(file, other.file), [file, other]);
  const counts = useMemo(() => {
    const out: Record<Status, number> = { changed: 0, there: 0, here: 0, same: 0 };
    for (const entry of pairs) out[entry.status]++;
    return out;
  }, [pairs]);
  const [filter, setFilter] = useState<Status | "differ">("differ");
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [shown, setShown] = useState<number | null>(null);
  const rows = pairs.map((entry, index) => ({ entry, index })).filter(({ entry }) => filter === "differ" ? entry.status !== "same" : entry.status === filter);
  const copyable = (entry: Pair) => entry.there !== null && entry.status !== "same";
  // Monsters a copied table would share with another table here (gs does not start then).
  const clashes = useMemo(() => {
    const owners = new Map<number, string>();
    file.tables.forEach((table) => table.monsters.forEach((id) => owners.set(id, table.name)));
    const out = new Map<number, number[]>();
    pairs.forEach((entry, index) => {
      if (entry.there === null) return;
      const ids = other.file.tables[entry.there].monsters.filter((id) => owners.has(id) && owners.get(id) !== entry.name);
      if (ids.length) out.set(index, ids);
    });
    return out;
  }, [file, other, pairs]);
  const chosen = pairs.filter((entry, index) => picked.has(index) && copyable(entry));
  const selected = shown !== null ? pairs[shown] : rows[0]?.entry ?? null;

  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal ai-sim-dialog ai-compare-dialog" role="dialog" aria-label="Compare" onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head"><h3>Compare with <span className="mono" title={other.path}>{other.path.split(/[\\/]/).pop()}</span></h3><span className="tag">version {other.file.version}</span><span className="spacer" /><button className="btn small" onClick={onChooseOther}><FolderOpen size={13} /> Other file…</button><button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button></header>
      <div className="npcgen-nearby-bar">
        <span className="dyn-compare-tabs">
          <button className={"btn small" + (filter === "differ" ? " active" : "")} onClick={() => setFilter("differ")}>Differ <span className="muted">{count(counts.changed + counts.there + counts.here)}</span></button>
          {(Object.keys(LABELS) as Status[]).map((status) => <button key={status} className={"btn small" + (filter === status ? " active" : "")} onClick={() => setFilter(status)}>{LABELS[status]} <span className="muted">{count(counts[status])}</span></button>)}
        </span>
        <span className="muted small">Tables pair by name.</span>
      </div>
      <div className="ai-cmp-body">
        <div className="ai-cmp-list">
          <div className="ai-cmp-list-head">
            <input type="checkbox" title="Pick every table shown that can be copied" checked={rows.some(({ entry }) => copyable(entry)) && rows.filter(({ entry }) => copyable(entry)).every(({ index }) => picked.has(index))} onChange={(event) => setPicked((current) => { const next = new Set(current); for (const { entry, index } of rows) if (copyable(entry)) { if (event.target.checked) next.add(index); else next.delete(index); } return next; })} />
            <span className="muted small">{count(rows.length)} tables</span>
          </div>
          {rows.map(({ entry, index }) => <div key={index} className={"ai-cmp-row drops-cmp-row" + (selected === entry ? " selected" : "")} onClick={() => setShown(index)}>
            {copyable(entry) ? <input type="checkbox" checked={picked.has(index)} onClick={(event) => event.stopPropagation()} onChange={() => setPicked((current) => { const next = new Set(current); if (next.has(index)) next.delete(index); else next.add(index); return next; })} /> : <span />}
            <span className="truncate">{entry.name || "(no name)"}</span>
            <span className={"tag ai-cmp-" + entry.status}>{LABELS[entry.status].toLowerCase()}</span>
            <span className="muted small truncate">{entry.parts.join(", ")}{clashes.has(index) && <span className="ai-warn"> · monsters in other tables</span>}</span>
          </div>)}
          {!rows.length && <div className="empty-note">No table.</div>}
        </div>
        <div className="ai-cmp-detail">
          {!selected ? <div className="empty-note">Select a table.</div> : <>
            <div className="ai-cmp-detail-head"><b>{selected.name}</b> <span className={"tag ai-cmp-" + selected.status}>{LABELS[selected.status].toLowerCase()}</span></div>
            {(() => { const index = pairs.indexOf(selected); const ids = clashes.get(index); return ids ? <p className="ai-warn small">Monsters {ids.slice(0, 8).join(", ")}{ids.length > 8 ? ", …" : ""} are in other tables here: copying would put them in two tables (gs does not start) until they are removed from one.</p> : null; })()}
            <div className="ai-cmp-pairs">
              <div className="ai-cmp-pair-head drops-cmp-head"><span>Here</span><span>{other.path.split(/[\\/]/).pop()}</span></div>
              <div className="ai-cmp-pair drops-cmp-pair">
                <TableSide table={selected.here !== null ? file.tables[selected.here] : null} nameOf={nameOf} />
                <TableSide table={selected.there !== null ? other.file.tables[selected.there] : null} nameOf={nameOf} />
              </div>
            </div>
          </>}
        </div>
      </div>
      <footer className="npcgen-nearby-foot">
        <span className="muted small">A copied table replaces the one of the same name here, or is added at the end. One undo step.</span>
        <span className="spacer" />
        <button className="btn" onClick={onClose}>Close</button>
        <button className="btn primary" disabled={!chosen.length} onClick={() => { onCopy(chosen.map((entry) => other.file.tables[entry.there!])); setPicked(new Set()); }}><Check size={14} /> Copy {count(chosen.length)} table{chosen.length === 1 ? "" : "s"}</button>
      </footer>
    </div>
  </div>;
}
