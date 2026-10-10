import { useEffect, useMemo, useState } from "react";
import { Copy, FolderOpen } from "lucide-react";
import { count } from "../elements/format";
import type { GenCompareRow, GenComparison, NpcGenSection } from "../elements/types";

const STATUS: Record<GenCompareRow["status"], string> = { missing: "Only in the other file", different: "Different", same: "Same", only_here: "Only in this file" };
const SECTION_NAMES: Record<NpcGenSection, string> = { areas: "Spawns", resources: "Resources", objects: "Objects", controllers: "Controllers" };
const ONE: Record<NpcGenSection, string> = { areas: "Spawn area", resources: "Resource area", objects: "Object", controllers: "Controller" };

/** Readable names of the stored fields that differ. */
const FIELDS: Record<string, string> = {
  kind: "placement", position: "position", direction: "direction", extents: "size", npcType: "type", groupType: "group type", initGen: "spawn at start", revive: "revive", validOnce: "valid once",
  genId: "generator ID", controller: "controller", lifeTime: "life time", maxCount: "max count", attachNum: "attachments", attached: "attachments", phase: "phase", generators: "generators",
  "generators.id": "NPC or monster", "generators.count": "count", "generators.refresh": "respawn", "generators.diedTimes": "death count", "generators.aggressive": "aggressive",
  "generators.offsetWater": "offsets", "generators.offsetTerrain": "offsets", "generators.faction": "factions", "generators.factionHelper": "factions", "generators.factionAccept": "factions",
  "generators.needHelp": "factions", "generators.defaultFaction": "factions", "generators.defaultFactionHelper": "factions", "generators.defaultFactionAccept": "factions",
  "generators.pathId": "path", "generators.loopType": "path type", "generators.speedFlag": "speed", "generators.deadTime": "corpse time",
  extentX: "size", extentZ: "size", autoRevive: "auto revive", resources: "resources", "resources.kind": "type", "resources.template": "mine", "resources.refresh": "respawn",
  "resources.count": "count", "resources.heightOffset": "height", radius: "rotation", id: "ID", scale: "scale", name: "name", controllerId: "trigger ID", active: "active at start",
  waitTime: "wait time", stopTime: "stop time", activeTimeInvalid: "dates", stopTimeInvalid: "dates", activeTime: "dates", stopTimeAt: "dates", activeTimeRange: "active range",
  repeat: "repeat", segmentLogic: "time segments", segments: "time segments",
};
const describe = (fields: string[]) => [...new Set(fields.map((field) => FIELDS[field] ?? field))].join(", ");

interface Props {
  comparison: GenComparison | null;
  version: number;
  busy: boolean;
  labels: Record<string, string>;
  onChoose: () => void;
  onCopy: (picks: [NpcGenSection, number][]) => void;
  onShow: (section: NpcGenSection, index: number) => void;
}

export function NpcGenCompare({ comparison, version, busy, labels, onChoose, onCopy, onShow }: Props) {
  const [status, setStatus] = useState<GenCompareRow["status"]>("missing");
  const [section, setSection] = useState<NpcGenSection | "all">("all");
  const [picked, setPicked] = useState<Set<string>>(new Set());
  useEffect(() => setPicked(new Set()), [comparison]);
  const key = (row: GenCompareRow) => `${row.section}:${row.there}`;
  const counts = useMemo(() => {
    const out: Record<string, number> = {};
    for (const row of comparison?.rows ?? []) {
      out[row.status] = (out[row.status] ?? 0) + 1;
      out[`${row.status}:${row.section}`] = (out[`${row.status}:${row.section}`] ?? 0) + 1;
    }
    return out;
  }, [comparison]);
  const rows = (comparison?.rows ?? []).filter((row) => row.status === status && (section === "all" || row.section === section));
  const copyable = (row: GenCompareRow) => (row.status === "missing" || row.status === "different") && !row.blocked && row.there !== null;
  const selectable = rows.filter(copyable);
  const toggle = (row: GenCompareRow) => setPicked((current) => {
    const next = new Set(current);
    if (next.has(key(row))) next.delete(key(row));
    else next.add(key(row));
    return next;
  });
  const pickedRows = (comparison?.rows ?? []).filter((row) => picked.has(key(row)));
  const what = (row: GenCompareRow) => {
    if (row.section === "controllers") return <><span className="mono">{row.ids[0]}</span> {row.label || <span className="muted">(no name)</span>}</>;
    if (row.section === "objects") return <>Object <span className="mono">{row.ids[0]}</span></>;
    if (!row.ids.length) return <span className="muted">(empty)</span>;
    const first = row.ids[0];
    const name = labels[String(first)]?.split(" › ").pop();
    return <><span className="mono">{first}</span> {name ?? ""}{row.ids.length > 1 && <span className="muted"> +{row.ids.length - 1}</span>}</>;
  };

  return <div className="npcgen-nearby">
    <div className="npcgen-nearby-bar">
      {comparison ? <span className="mono truncate" title={comparison.path}>{comparison.path}</span> : <span className="muted">Choose another server's npcgen.data for the same map, or a JSON export.</span>}
      {comparison && /\.json$/i.test(comparison.path) && <span className="path-data-badge"><b>JSON export</b></span>}
      {comparison && <span className="path-data-badge"><b>Version:</b> {comparison.version}{comparison.version !== version ? ` (this file: ${version})` : ""}</span>}
      <span className="spacer" />
      <button className="btn" onClick={onChoose} disabled={busy}><FolderOpen size={14} /> {comparison ? "Choose another…" : "Choose npcgen.data or JSON…"}</button>
    </div>
    {!comparison ? <div className="empty-note">Items are paired by what they spawn and where: spawn areas by type and NPCs or monsters, resource areas by mines, objects by object ID, each at the same place or the nearest within 10 m; controllers by their ID. You can copy what this file lacks and replace what differs. Copied items get new export IDs where theirs are taken here, attachments follow the pairing, the controllers they use come along, and values this file's version cannot store are cleared.</div> : <>
      <div className="npcgen-nearby-bar">
        <span className="dyn-compare-tabs">{(Object.keys(STATUS) as GenCompareRow["status"][]).map((entry) => <button key={entry} className={"btn small" + (status === entry ? " active" : "")} onClick={() => { setStatus(entry); setPicked(new Set()); }}>{STATUS[entry]} <span className="muted">{count(counts[entry] ?? 0)}</span></button>)}</span>
        <span className="spacer" />
        <span className="dyn-compare-tabs">{(["all", "areas", "resources", "objects", "controllers"] as const).map((entry) => <button key={entry} className={"btn small" + (section === entry ? " active" : "")} onClick={() => setSection(entry)}>{entry === "all" ? "All" : SECTION_NAMES[entry]} <span className="muted">{count(entry === "all" ? counts[status] ?? 0 : counts[`${status}:${entry}`] ?? 0)}</span></button>)}</span>
      </div>
      <div className="npcgen-nearby-table">
        {rows.length ? <table className="dyn-table">
          <thead><tr>
            <th>{selectable.length > 0 && <input type="checkbox" checked={selectable.every((row) => picked.has(key(row)))} title="Select every row shown" onChange={(event) => setPicked((current) => { const next = new Set(current); for (const row of selectable) { if (event.target.checked) next.add(key(row)); else next.delete(key(row)); } return next; })} />}</th>
            <th>Item</th><th>Spawns</th><th>X</th><th>Z</th><th>{status === "different" ? "Differs in" : ""}</th><th />
          </tr></thead>
          <tbody>{rows.map((row) => <tr key={`${row.section}:${row.there}:${row.here}`} className={picked.has(key(row)) ? "selected" : undefined}>
            <td>{copyable(row) && <input type="checkbox" checked={picked.has(key(row))} onChange={() => toggle(row)} />}</td>
            <td className="nowrap">{ONE[row.section]} {row.here !== null ? <span className="muted small">{row.here + 1} here</span> : <span className="muted small">{(row.there ?? 0) + 1} there</span>}</td>
            <td className="truncate">{what(row)}</td>
            <td className="mono">{row.section === "controllers" ? "" : row.x.toFixed(1)}</td>
            <td className="mono">{row.section === "controllers" ? "" : row.z.toFixed(1)}</td>
            <td className="muted small">{row.blocked ?? describe(row.fields)}</td>
            <td>{row.here !== null && <button className="btn small" onClick={() => onShow(row.section, row.here!)}>Show</button>}</td>
          </tr>)}</tbody>
        </table> : <div className="empty-note">Nothing here.</div>}
      </div>
      <footer className="npcgen-nearby-foot">
        <span className="muted small">{status === "different" ? "Replacing keeps this file's export ID." : status === "missing" ? "Copies go to the end of their section." : ""}</span>
        <span className="spacer" />
        <button className="btn primary" disabled={!pickedRows.length || busy} onClick={() => onCopy(pickedRows.map((row) => [row.section, row.there!]))}><Copy size={14} /> Copy {count(pickedRows.length)} item{pickedRows.length === 1 ? "" : "s"}</button>
      </footer>
    </>}
  </div>;
}
