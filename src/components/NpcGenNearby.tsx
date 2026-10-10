import { useEffect, useMemo, useRef, useState } from "react";
import { Combine, Download, Eraser, Loader2, RefreshCw, Split } from "lucide-react";
import { gameNearby } from "../elements/api";
import { count } from "../elements/format";
import type { NearbyClass, NearbyImport, NpcGenView } from "../elements/types";
import { NumberInput } from "./DynTasksEditor";

/** A thing seen in the game client, kept across fetches until the list is cleared. */
export interface CollectedRow {
  key: string;
  runtimeId: number;
  class: NearbyClass;
  template: number;
  label: string | null;
  position: { x: number; y: number; z: number };
  direction: { x: number; y: number; z: number } | null;
  rotation: [number, number, number] | null;
  phase: number | null;
  /** How many fetches saw it. */
  seen: number;
  /** A group: the monsters or mines it merges (single rows), and the full size of the area they cover. */
  members?: CollectedRow[];
  size?: { x: number; z: number };
}

export interface NearbyState {
  rows: CollectedRow[];
  player: { x: number; y: number; z: number } | null;
  selected: string[];
  shown: Record<"npc" | "monster" | "mine" | "dynamic", boolean>;
  radius: number;
  count: number;
  refresh: number;
  /** Monsters or mines of one template this close to each other form a group. */
  groupRadius: number;
}

export const emptyNearby = (): NearbyState => ({ rows: [], player: null, selected: [], shown: { npc: true, monster: true, mine: true, dynamic: true }, radius: 0, count: 1, refresh: 60, groupRadius: 10 });

const CLASS_NAMES: Record<NearbyClass, string> = { npc: "NPC", monster: "Monster", unknown: "NPC or monster", mine: "Mine", dynamic: "Object", item: "Dropped item" };
export const CLASS_COLORS: Record<NearbyClass, string> = { npc: "#3b82c4", monster: "#d9534f", unknown: "#c08a2e", mine: "#2e9e5b", dynamic: "#888", item: "#888" };
const showKey = (kind: NearbyClass) => (kind === "unknown" ? "npc" : kind) as keyof NearbyState["shown"];

const distance = (a: { x: number; z: number }, b: { x: number; z: number }) => Math.hypot(a.x - b.x, a.z - b.z);

/** Facing in compass degrees (0 = north), from a direction or an upright rotation. */
function facing(row: CollectedRow): number | null {
  if (row.class === "mine" || row.class === "dynamic") {
    if (!row.rotation || row.rotation[0] !== 0 || row.rotation[1] !== 0) return null;
    return Math.round((row.rotation[2] / 255) * 360) % 360;
  }
  if (!row.direction || Math.hypot(row.direction.x, row.direction.z) < 1e-3) return null;
  return Math.round(((Math.atan2(row.direction.x, row.direction.z) * 180) / Math.PI + 360) % 360);
}

/** Already in the open file: the same template at (about) the same place. */
export function inFile(view: NpcGenView, row: CollectedRow): boolean {
  const near = (entries: NpcGenView["areas"], template: number) => entries.some((entry) => entry.ids.includes(template) && distance(entry, row.position) <= Math.max(2, entry.extX / 2, entry.extZ / 2));
  if (row.class === "mine") return near(view.resources, row.template);
  if (row.class === "dynamic") return near(view.objects, row.template);
  return near(view.areas, row.template);
}

/** Space around the outermost members of a group (on each side). */
const GROUP_MARGIN = 2;

/** One row for several monsters or mines: centred on them, sized to cover them. */
function makeGroup(members: CollectedRow[]): CollectedRow {
  const xs = members.map((row) => row.position.x), zs = members.map((row) => row.position.z);
  const [minX, maxX, minZ, maxZ] = [Math.min(...xs), Math.max(...xs), Math.min(...zs), Math.max(...zs)];
  const first = members[0];
  return {
    ...first,
    key: `group:${first.key}`,
    position: { x: (minX + maxX) / 2, y: members.reduce((total, row) => total + row.position.y, 0) / members.length, z: (minZ + maxZ) / 2 },
    direction: null,
    seen: Math.max(...members.map((row) => row.seen)),
    members,
    size: { x: Math.max(4, maxX - minX + GROUP_MARGIN * 2), z: Math.max(4, maxZ - minZ + GROUP_MARGIN * 2) },
  };
}

/** Groups monsters and mines of one template that stand within `radius` of each other (chains count). */
export function groupRows(rows: CollectedRow[], radius: number): CollectedRow[] {
  const out: CollectedRow[] = [];
  const byTemplate = new Map<string, CollectedRow[]>();
  for (const row of rows) {
    if (row.class !== "monster" && row.class !== "mine") out.push(row);
    else {
      const key = `${row.class}:${row.template}`;
      byTemplate.set(key, [...(byTemplate.get(key) ?? []), ...(row.members ?? [row])]);
    }
  }
  for (const singles of byTemplate.values()) {
    const parent = singles.map((_, index) => index);
    const root = (index: number): number => (parent[index] === index ? index : (parent[index] = root(parent[index])));
    for (let a = 0; a < singles.length; a++) {
      for (let b = a + 1; b < singles.length; b++) {
        if (distance(singles[a].position, singles[b].position) <= radius) parent[root(a)] = root(b);
      }
    }
    const clusters = new Map<number, CollectedRow[]>();
    singles.forEach((row, index) => clusters.set(root(index), [...(clusters.get(root(index)) ?? []), row]));
    for (const members of clusters.values()) out.push(members.length > 1 ? makeGroup(members) : members[0]);
  }
  return out;
}

export const ungroupRows = (rows: CollectedRow[]) => rows.flatMap((row) => row.members ?? [row]);

/** Adds a fetch to the list: by runtime ID, or a row of the same template within a metre (it respawned). */
function merge(rows: CollectedRow[], fetched: Awaited<ReturnType<typeof gameNearby>>): CollectedRow[] {
  const next = rows.map((row) => (row.members ? { ...row, members: row.members.map((member) => ({ ...member })) } : { ...row }));
  // Single rows and group members, with the group each belongs to.
  const leaves = next.flatMap((row) => (row.members ?? [row]).map((leaf) => ({ leaf, group: row.members ? row : null })));
  const byId = new Map(leaves.map((entry) => [entry.leaf.runtimeId, entry]));
  for (const entity of fetched.rows) {
    if (entity.class === "item") continue;
    const update = { runtimeId: entity.runtimeId, class: entity.class, template: entity.template, label: entity.label, position: entity.position, direction: entity.direction, rotation: entity.rotation, phase: entity.phase };
    const known = byId.get(entity.runtimeId) ?? leaves.find((entry) => entry.leaf.template === entity.template && entry.leaf.class === entity.class && distance(entry.leaf.position, entity.position) < 1);
    if (known) {
      // Spawns that stand still keep their first position; wandering monsters keep where they were first seen.
      const { leaf, group } = known;
      Object.assign(leaf, { ...update, position: leaf.position, direction: leaf.direction ?? entity.direction, seen: leaf.seen + 1 });
      if (group) group.seen = Math.max(group.seen, leaf.seen);
      byId.set(entity.runtimeId, known);
    } else {
      const row: CollectedRow = { key: `${entity.runtimeId}:${entity.template}`, ...update, seen: 1 };
      next.push(row);
      const entry = { leaf: row, group: null };
      leaves.push(entry);
      byId.set(entity.runtimeId, entry);
    }
  }
  return next;
}

interface Props {
  view: NpcGenView;
  state: NearbyState;
  setState: (update: (state: NearbyState) => NearbyState) => void;
  elementsOpen: boolean;
  busy: boolean;
  /** The running client to read (asks when several run; `choose` asks again); throws with a message. */
  pickClient: (choose: boolean) => Promise<number | null>;
  onImport: (rows: NearbyImport[], options: { count: number; refresh: number }) => Promise<boolean>;
  onShow: (row: CollectedRow) => void;
}

export function NearbyPanel({ view, state, setState, elementsOpen, busy, pickClient, onImport, onShow }: Props) {
  const [fetching, setFetching] = useState(false);
  const [auto, setAuto] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fetchingRef = useRef(false);

  const fetchNow = async (choose: boolean) => {
    if (fetchingRef.current) return;
    fetchingRef.current = true;
    setFetching(true);
    setError(null);
    try {
      const pid = await pickClient(choose);
      if (pid === null) return;
      const fetched = await gameNearby(pid);
      setState((current) => ({ ...current, rows: merge(current.rows, fetched), player: { x: fetched.player.x, y: fetched.player.y, z: fetched.player.z } }));
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
      setAuto(false);
    } finally {
      fetchingRef.current = false;
      setFetching(false);
    }
  };
  const fetchLatest = useRef(fetchNow);
  fetchLatest.current = fetchNow;
  useEffect(() => {
    if (!auto) return;
    const timer = window.setInterval(() => void fetchLatest.current(false), 5000);
    return () => window.clearInterval(timer);
  }, [auto]);

  const shown = useMemo(() => state.rows
    .filter((row) => state.shown[showKey(row.class)] && (!state.radius || !state.player || distance(row.position, state.player) <= state.radius))
    .map((row) => ({ row, distance: state.player ? distance(row.position, state.player) : null, inFile: inFile(view, row) }))
    .sort((a, b) => (a.distance ?? 0) - (b.distance ?? 0)), [state.player, state.radius, state.rows, state.shown, view]);
  const selected = new Set(state.selected);
  const importable = (row: CollectedRow) => row.class !== "unknown";
  const selectedShown = shown.filter((entry) => selected.has(entry.row.key));
  const newShown = shown.filter((entry) => !entry.inFile && importable(entry.row));
  const unknown = state.rows.some((row) => row.class === "unknown");
  const toggle = (keys: string[], on: boolean) => setState((current) => {
    const next = new Set(current.selected);
    for (const key of keys) {
      if (on) next.add(key);
      else next.delete(key);
    }
    return { ...current, selected: [...next] };
  });
  const allSelected = newShown.length > 0 && newShown.every((entry) => selected.has(entry.row.key));

  const importRows = async (rows: CollectedRow[]) => {
    const list = rows.filter(importable).map((row): NearbyImport => ({ kind: row.class as NearbyImport["kind"], template: row.template, position: row.position, direction: row.direction, rotation: row.rotation, phase: row.phase, size: row.size ? [row.size.x, row.size.z] : null, members: row.members?.length ?? null }));
    if (!list.length) return;
    if (await onImport(list, { count: state.count, refresh: state.refresh })) toggle(rows.map((row) => row.key), false);
  };

  const phaseKnown = state.rows.some((row) => row.phase !== null);
  const grouped = state.rows.some((row) => row.members);
  const regroup = (rows: CollectedRow[]) => setState((current) => {
    const keys = new Set(rows.map((row) => row.key));
    return { ...current, rows, selected: current.selected.filter((key) => keys.has(key)) };
  });
  return <div className="npcgen-nearby">
    <div className="npcgen-nearby-bar">
      <button className="btn primary" disabled={fetching} onClick={(event) => void fetchNow(event.shiftKey)} title="Read what the running game client has loaded around your character (Shift+click: choose the client when several run)">
        {fetching ? <Loader2 size={14} className="spin" /> : <RefreshCw size={14} />} Fetch
      </button>
      <label className="npcgen-flag" title="Fetch again every 5 seconds while you walk around"><input type="checkbox" checked={auto} onChange={(event) => setAuto(event.target.checked)} /> Every 5 s</label>
      <span className="npcgen-nearby-filters">
        {(["npc", "monster", "mine", "dynamic"] as const).map((kind) => <label key={kind} className="npcgen-flag"><input type="checkbox" checked={state.shown[kind]} onChange={(event) => setState((current) => ({ ...current, shown: { ...current.shown, [kind]: event.target.checked } }))} /><i className="npcgen-dot" style={{ background: CLASS_COLORS[kind] }} />{kind === "dynamic" ? "Objects" : `${CLASS_NAMES[kind]}s`}</label>)}
      </span>
      <label className="npcgen-flag" title="Only rows this close to where you stood at the last fetch (0: everything collected)">Within <NumberInput value={state.radius} onCommit={(radius) => setState((current) => ({ ...current, radius }))} /> m</label>
      <span className="spacer" />
      <button className="btn" disabled={!state.rows.length} onClick={() => setState((current) => ({ ...current, rows: [], selected: [] }))}><Eraser size={14} /> Clear list</button>
    </div>
    <div className="npcgen-nearby-bar">
      <label className="npcgen-flag" title="Monsters or mines of the same template this close to each other (chains count) become one area">Group within <NumberInput min={1} value={state.groupRadius} onCommit={(groupRadius) => setState((current) => ({ ...current, groupRadius }))} /> m</label>
      <button className="btn" disabled={!state.rows.some((row) => row.class === "monster" || row.class === "mine")} onClick={() => regroup(groupRows(state.rows, state.groupRadius))} title="Merge monsters and mines of one template standing close together into areas (again after more fetches)"><Combine size={14} /> Group monsters and mines</button>
      <button className="btn" disabled={!grouped} onClick={() => regroup(ungroupRows(state.rows))}><Split size={14} /> Ungroup</button>
      <span className="muted small">Groups import as areas covering them, spawning as many as were seen; single rows as points.</span>
    </div>
    {error && <div className="path-data-message error">{error}</div>}
    {!elementsOpen && <div className="path-data-message">Open the server's elements.data to see names and to tell NPCs from monsters; until then they can be collected but not imported.</div>}
    {elementsOpen && unknown && <div className="path-data-message">Some IDs are not in the open elements.data (orange); they cannot be imported as NPCs or monsters.</div>}
    <div className="npcgen-nearby-table">
      {state.rows.length ? <table className="dyn-table">
        <thead><tr>
          <th><input type="checkbox" checked={allSelected} disabled={!newShown.length} title="Select every new row shown" onChange={(event) => toggle(newShown.map((entry) => entry.row.key), event.target.checked)} /></th>
          <th>Kind</th><th>ID</th><th>Name</th><th>X</th><th>Y</th><th>Z</th><th title="Compass degrees, 0 = north">Facing</th><th title="From where you stood at the last fetch">Distance</th>
          <th title={phaseKnown ? "Phase (0: everyone sees it)" : "Found once phased NPCs or mines are in view"}>Phase</th><th title="How many a group merges">Count</th><th title="Groups: the area they cover (width × depth)">Size</th><th>Status</th><th />
        </tr></thead>
        <tbody>{shown.map(({ row, distance: away, inFile: present }) => {
          const angle = facing(row);
          return <tr key={row.key} className={(selected.has(row.key) ? "selected " : "") + (present ? "npcgen-nearby-known" : "")} onClick={() => onShow(row)}>
            <td onClick={(event) => event.stopPropagation()}><input type="checkbox" checked={selected.has(row.key)} disabled={!importable(row)} onChange={(event) => toggle([row.key], event.target.checked)} /></td>
            <td><span className="npcgen-flag"><i className="npcgen-dot" style={{ background: CLASS_COLORS[row.class] }} />{CLASS_NAMES[row.class]}</span></td>
            <td className="mono">{row.template}</td>
            <td className="truncate" title={row.label ?? undefined}>{row.label?.split(" › ").pop() ?? <span className="muted">—</span>}</td>
            <td className="mono">{row.position.x.toFixed(1)}</td><td className="mono">{row.position.y.toFixed(1)}</td><td className="mono">{row.position.z.toFixed(1)}</td>
            <td className="mono">{angle === null ? <span className="muted">—</span> : `${angle}°`}</td>
            <td className="mono">{away === null ? "" : `${away.toFixed(1)} m`}</td>
            <td className="mono">{row.phase ?? <span className="muted">?</span>}</td>
            <td className="mono">{row.members?.length ?? 1}</td>
            <td className="mono">{row.size ? `${row.size.x.toFixed(0)}×${row.size.z.toFixed(0)} m` : <span className="muted">point</span>}</td>
            <td>{present ? <span className="muted small">In file</span> : <span className="tag ok">New</span>}</td>
            <td onClick={(event) => event.stopPropagation()}><button className="btn small" disabled={busy || !importable(row)} title={importable(row) ? "Add it to the file" : "Not in the open elements.data"} onClick={() => void importRows([row])}><Download size={12} /> Import</button></td>
          </tr>;
        })}</tbody>
      </table> : <div className="empty-note">Stand in the game where you want to collect and click Fetch. NPCs, monsters, mines and objects your client has loaded (about 100–200 m around you) are listed here and stay until you clear the list, so you can walk on and fetch again.</div>}
    </div>
    <footer className="npcgen-nearby-foot">
      <span className="muted small">{count(state.rows.length)} collected · {count(shown.length)} shown · {count(shown.length - newShown.length)} in the file or not importable</span>
      <span className="spacer" />
      <label className="npcgen-flag" title="Monsters and mines per imported spawn (NPCs: always one)">Count <NumberInput min={1} value={state.count} onCommit={(value) => setState((current) => ({ ...current, count: value }))} /></label>
      <label className="npcgen-flag" title="Respawn time for imported monsters and mines, in seconds (the server adds 15 s; NPCs: 0)">Respawn <NumberInput value={state.refresh} onCommit={(value) => setState((current) => ({ ...current, refresh: value }))} /> s</label>
      <button className="btn" disabled={busy || !selectedShown.length} onClick={() => void importRows(selectedShown.map((entry) => entry.row))}><Download size={14} /> Import selected ({count(selectedShown.length)})</button>
      <button className="btn primary" disabled={busy || !newShown.length} onClick={() => void importRows(newShown.map((entry) => entry.row))} title="Every new row shown (filters and distance apply)"><Download size={14} /> Import all new ({count(newShown.length)})</button>
    </footer>
  </div>;
}
