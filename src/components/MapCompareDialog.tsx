import { useMemo, useState } from "react";
import { Check, FolderOpen, X } from "lucide-react";
import { count } from "../elements/format";
import type { ClientMap, MapPrecinct, MapRegionFile, MapRegionItem } from "../elements/types";

type Status = "changed" | "there" | "here" | "same";
type Mode = "whole" | "names";
const LABELS: Record<Status, string> = { changed: "Changed", there: "Only there", here: "Only here", same: "Same" };

/** A precinct or a region file entry, paired between the two files. */
interface Pair {
  key: string;
  label: string;
  here: number | null;
  there: number | null;
  status: Status;
  parts: string[];
  /** Names differ (names-only mode can take them). */
  names: boolean;
}

interface Props {
  path: string;
  other: MapRegionFile;
  file: MapRegionFile;
  maps: ClientMap[];
  onApply: (next: MapRegionFile, message: string) => void;
  onChooseOther: () => void;
  onClose: () => void;
}

const near = (a: number, b: number) => Math.abs(a - b) < 0.01;
const sameVec = (a: { x: number; y: number; z: number }, b: { x: number; y: number; z: number }) => near(a.x, b.x) && near(a.y, b.y) && near(a.z, b.z);
const samePoints = (a: { x: number; y: number; z: number }[], b: { x: number; y: number; z: number }[]) => a.length === b.length && a.every((point, index) => sameVec(point, b[index]));

function precinctParts(a: MapPrecinct, b: MapPrecinct): string[] {
  const parts: string[] = [];
  if (a.name !== b.name) parts.push("name");
  if (!samePoints(a.points, b.points)) parts.push("outline");
  if (a.priority !== b.priority || a.instance !== b.instance || a.domain !== b.domain || a.id !== b.id) parts.push("settings");
  if (a.cityInstance !== b.cityInstance || !sameVec(a.city, b.city)) parts.push("return city");
  if (a.rebirths !== b.rebirths || a.levelMin !== b.levelMin || a.levelMax !== b.levelMax || a.kickCity !== b.kickCity || !sameVec(a.kickPos, b.kickPos)) parts.push("level limits");
  if (a.marks.length !== b.marks.length || a.marks.some((mark, index) => mark.name !== b.marks[index].name || !sameVec(mark.pos, b.marks[index].pos))) parts.push("marks");
  if (a.sound !== b.sound || a.soundNight !== b.soundNight || a.minimap !== b.minimap || a.music.join("|") !== b.music.join("|") || a.musicInterval !== b.musicInterval || a.musicLoop !== b.musicLoop) parts.push("sound");
  return parts;
}

function itemParts(a: MapRegionItem, b: MapRegionItem): string[] {
  if (a.kind === "region" && b.kind === "region") return [a.name !== b.name ? "name" : "", a.regionType !== b.regionType ? "type" : "", !samePoints(a.points, b.points) ? "outline" : ""].filter(Boolean);
  if (a.kind === "portal" && b.kind === "portal") return [!sameVec(a.pos, b.pos) || !sameVec(a.extents, b.extents) ? "box" : "", !sameVec(a.targetPos, b.targetPos) ? "arrival" : "", a.level !== b.level || a.line !== b.line || a.controller !== b.controller ? "settings" : ""].filter(Boolean);
  if (a.kind === "guildBuilding" && b.kind === "guildBuilding") return JSON.stringify(a) === JSON.stringify(b) ? [] : ["settings"];
  return ["kind"];
}

/** Pairs entries: precincts by ID (else name and instance), regions by name, portals by from → to, buildings by template
 * and position; repeats pair in order. */
function pair(file: MapRegionFile, other: MapRegionFile, mapName: (id: number) => string): Pair[] {
  const out: Pair[] = [];
  const queues = new Map<string, number[]>();
  if (file.kind === "precinct" && other.kind === "precinct") {
    const keyOf = (p: MapPrecinct) => p.id ? `id:${p.id}` : `name:${p.instance}:${p.name}`;
    other.precincts.forEach((p, index) => queues.set(keyOf(p), [...(queues.get(keyOf(p)) ?? []), index]));
    file.precincts.forEach((p, index) => {
      const there = queues.get(keyOf(p))?.shift() ?? null;
      const parts = there === null ? [] : precinctParts(p, other.precincts[there]);
      const names = there !== null && (p.name !== other.precincts[there].name || p.marks.some((mark, at) => other.precincts[there].marks[at] && mark.name !== other.precincts[there].marks[at].name));
      out.push({ key: `h${index}`, label: `${p.name || "(no name)"}${p.id ? ` · ID ${p.id}` : ""}`, here: index, there, status: there === null ? "here" : parts.length ? "changed" : "same", parts, names });
    });
    for (const rest of queues.values()) for (const index of rest) { const p = other.precincts[index]; out.push({ key: `t${index}`, label: `${p.name || "(no name)"}${p.id ? ` · ID ${p.id}` : ""}`, here: null, there: index, status: "there", parts: [], names: false }); }
    return out;
  }
  if (file.kind !== "region" || other.kind !== "region") return out;
  const keyOf = (item: MapRegionItem) => item.kind === "region" ? `r:${item.name}` : item.kind === "portal" ? `p:${item.instance}:${item.target}` : `g:${item.template}:${Math.round(item.pos.x)}:${Math.round(item.pos.z)}`;
  const label = (item: MapRegionItem) => item.kind === "region" ? `Region “${item.name}”` : item.kind === "portal" ? `Portal ${mapName(item.instance)} → ${mapName(item.target)}` : `Guild building ${item.template}`;
  other.items.forEach((item, index) => queues.set(keyOf(item), [...(queues.get(keyOf(item)) ?? []), index]));
  file.items.forEach((item, index) => {
    const there = queues.get(keyOf(item))?.shift() ?? null;
    const parts = there === null ? [] : itemParts(item, other.items[there]);
    out.push({ key: `h${index}`, label: label(item), here: index, there, status: there === null ? "here" : parts.length ? "changed" : "same", parts, names: false });
  });
  for (const rest of queues.values()) for (const index of rest) out.push({ key: `t${index}`, label: label(other.items[index]), here: null, there: index, status: "there", parts: [], names: false });
  // Unnamed regions pair by position in the file when names differ (translations rename them).
  const regionsHere = out.filter((entry) => entry.status === "here" && file.items[entry.here!].kind === "region");
  const regionsThere = out.filter((entry) => entry.status === "there" && other.items[entry.there!].kind === "region");
  regionsHere.forEach((entry, at) => {
    const match = regionsThere[at];
    if (!match) return;
    const a = file.items[entry.here!], b = other.items[match.there!];
    entry.there = match.there;
    entry.parts = itemParts(a, b);
    entry.status = entry.parts.length ? "changed" : "same";
    entry.names = a.kind === "region" && b.kind === "region" && a.name !== b.name;
    out.splice(out.indexOf(match), 1);
  });
  return out;
}

/** The open map file against another copy of it (another client or server set): copy whole entries or only names. */
export function MapCompareDialog({ path, other, file, maps, onApply, onChooseOther, onClose }: Props) {
  const mapName = (id: number) => maps.find((map) => map.id === id)?.name ?? `instance ${id}`;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const pairs = useMemo(() => pair(file, other, mapName), [file, other, maps]);
  const [mode, setMode] = useState<Mode>("whole");
  const [filter, setFilter] = useState<Status | "differ">("differ");
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [shown, setShown] = useState<string | null>(null);
  const counts = useMemo(() => {
    const out: Record<Status, number> = { changed: 0, there: 0, here: 0, same: 0 };
    for (const entry of pairs) out[entry.status]++;
    return out;
  }, [pairs]);
  const copyable = (entry: Pair) => mode === "names" ? entry.names : entry.there !== null && entry.status !== "same";
  const rows = pairs.filter((entry) => filter === "differ" ? entry.status === "changed" || entry.status === "there" : entry.status === filter);
  const chosen = pairs.filter((entry) => picked.has(entry.key) && copyable(entry));
  const selected = pairs.find((entry) => entry.key === shown) ?? rows[0] ?? null;
  const name = path.split(/[\\/]/).pop();

  const apply = () => {
    if (file.kind === "precinct" && other.kind === "precinct") {
      const precincts = [...file.precincts];
      for (const entry of chosen) {
        const source = other.precincts[entry.there!];
        if (mode === "names") {
          const target = precincts[entry.here!];
          precincts[entry.here!] = { ...target, name: source.name, marks: target.marks.map((mark, at) => source.marks[at] ? { ...mark, name: source.marks[at].name } : mark) };
        } else if (entry.here !== null) precincts[entry.here] = structuredClone(source);
        else precincts.push(structuredClone(source));
      }
      onApply({ ...file, precincts }, `${mode === "names" ? "Took the names of" : "Copied"} ${chosen.length} area${chosen.length === 1 ? "" : "s"} from ${name}.`);
    } else if (file.kind === "region" && other.kind === "region") {
      const items = [...file.items];
      for (const entry of chosen) {
        const source = other.items[entry.there!];
        if (mode === "names") {
          const target = items[entry.here!];
          if (target.kind === "region" && source.kind === "region") items[entry.here!] = { ...target, name: source.name };
        } else if (entry.here !== null) items[entry.here] = structuredClone(source);
        else items.push(structuredClone(source));
      }
      onApply({ ...file, items }, `${mode === "names" ? "Took the names of" : "Copied"} ${chosen.length} entr${chosen.length === 1 ? "y" : "ies"} from ${name}.${mode === "whole" && chosen.some((entry) => entry.here === null && other.kind === "region" && other.items[entry.there!].kind === "portal") ? " New portals are added at the end (their numbers follow the existing ones)." : ""}`);
    }
    setPicked(new Set());
  };

  const describe = (source: MapRegionFile, index: number | null) => {
    if (index === null) return <div className="ai-cmp-side empty muted small">—</div>;
    if (source.kind === "precinct") {
      const p = source.precincts[index];
      return <div className="ai-cmp-side small">
        <div><b>{p.name}</b> · ID {p.id} · priority {p.priority} · instance {p.instance}</div>
        <div className="muted">{p.points.length} points · return {mapName(p.cityInstance)} ({p.city.x.toFixed(0)}, {p.city.z.toFixed(0)}){p.levelMin || p.levelMax ? ` · levels ${p.levelMin}–${p.levelMax || "∞"} (+${p.rebirths} rebirth)` : ""}</div>
        <div className="muted">Marks: {p.marks.map((mark) => mark.name).join(", ") || "none"}</div>
        <div className="muted">Sound: {p.sound || "—"}{p.music.length ? ` · music ${p.music.join(", ")}` : ""}</div>
      </div>;
    }
    const item = source.items[index];
    return <div className="ai-cmp-side small">{item.kind === "region" ? <><b>{item.name}</b> · {item.regionType === 0 ? "PK" : "safe zone"} · {item.points.length} points</>
      : item.kind === "portal" ? <><b>{mapName(item.instance)} → {mapName(item.target)}</b><div className="muted">box ({item.pos.x.toFixed(1)}, {item.pos.z.toFixed(1)}) ± ({item.extents.x}, {item.extents.z}) · arrival ({item.targetPos.x.toFixed(0)}, {item.targetPos.z.toFixed(0)}) · level {item.level} · controller {item.controller}</div></>
      : <>Guild building {item.template} at ({item.pos.x.toFixed(0)}, {item.pos.z.toFixed(0)})</>}</div>;
  };

  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal ai-sim-dialog ai-compare-dialog" role="dialog" aria-label="Compare" onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head"><h3>Compare with <span className="mono" title={path}>{name}</span></h3><span className="tag">version {other.version}</span><span className="spacer" /><button className="btn small" onClick={onChooseOther}><FolderOpen size={13} /> Other file…</button><button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button></header>
      <div className="npcgen-nearby-bar">
        <span className="dyn-compare-tabs">
          <button className={"btn small" + (filter === "differ" ? " active" : "")} onClick={() => setFilter("differ")}>Differ <span className="muted">{count(counts.changed + counts.there)}</span></button>
          {(Object.keys(LABELS) as Status[]).map((status) => <button key={status} className={"btn small" + (filter === status ? " active" : "")} onClick={() => setFilter(status)}>{LABELS[status]} <span className="muted">{count(counts[status])}</span></button>)}
        </span>
        <span className="spacer" />
        <span className="dyn-compare-tabs" title="What a picked entry takes from the other file">
          <button className={"btn small" + (mode === "whole" ? " active" : "")} onClick={() => setMode("whole")}>Whole entries</button>
          <button className={"btn small" + (mode === "names" ? " active" : "")} onClick={() => setMode("names")} title="Area names and their mark names, region names (translations)">Names only</button>
        </span>
      </div>
      <div className="ai-cmp-body">
        <div className="ai-cmp-list">
          <div className="ai-cmp-list-head">
            <input type="checkbox" checked={rows.some(copyable) && rows.filter(copyable).every((entry) => picked.has(entry.key))} onChange={(event) => setPicked((current) => { const next = new Set(current); for (const entry of rows) if (copyable(entry)) { if (event.target.checked) next.add(entry.key); else next.delete(entry.key); } return next; })} />
            <span className="muted small">{count(rows.length)} entries</span>
          </div>
          {rows.map((entry) => <div key={entry.key} className={"ai-cmp-row drops-cmp-row" + (selected === entry ? " selected" : "")} onClick={() => setShown(entry.key)}>
            {copyable(entry) ? <input type="checkbox" checked={picked.has(entry.key)} onClick={(event) => event.stopPropagation()} onChange={() => setPicked((current) => { const next = new Set(current); if (next.has(entry.key)) next.delete(entry.key); else next.add(entry.key); return next; })} /> : <span />}
            <span className="truncate">{entry.label}</span>
            <span className={"tag ai-cmp-" + entry.status}>{LABELS[entry.status].toLowerCase()}</span>
            <span className="muted small truncate">{entry.parts.join(", ")}</span>
          </div>)}
          {!rows.length && <div className="empty-note">Nothing.</div>}
        </div>
        <div className="ai-cmp-detail">
          {!selected ? <div className="empty-note">Select an entry.</div> : <>
            <div className="ai-cmp-detail-head"><b>{selected.label}</b> <span className={"tag ai-cmp-" + selected.status}>{LABELS[selected.status].toLowerCase()}</span></div>
            <div className="ai-cmp-pairs">
              <div className="ai-cmp-pair-head drops-cmp-head"><span>Here</span><span>{name}</span></div>
              <div className="ai-cmp-pair drops-cmp-pair">{describe(file, selected.here)}{describe(other, selected.there)}</div>
            </div>
          </>}
        </div>
      </div>
      <footer className="npcgen-nearby-foot">
        <span className="muted small">{mode === "names" ? "Names only: area names (and their marks' names, by position in the list), region names." : "Whole entries replace their pair here or are added at the end."} One undo step; saving writes the server file too.</span>
        <span className="spacer" />
        <button className="btn" onClick={onClose}>Close</button>
        <button className="btn primary" disabled={!chosen.length} onClick={apply}><Check size={14} /> {mode === "names" ? "Take names of" : "Copy"} {count(chosen.length)}</button>
      </footer>
    </div>
  </div>;
}
