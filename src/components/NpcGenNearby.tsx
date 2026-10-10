import { useEffect, useMemo, useRef, useState } from "react";
import { Download, Eraser, Loader2, RefreshCw } from "lucide-react";
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
}

export interface NearbyState {
  rows: CollectedRow[];
  player: { x: number; y: number; z: number } | null;
  selected: string[];
  shown: Record<"npc" | "monster" | "mine" | "dynamic", boolean>;
  radius: number;
  count: number;
  refresh: number;
}

export const emptyNearby = (): NearbyState => ({ rows: [], player: null, selected: [], shown: { npc: true, monster: true, mine: true, dynamic: true }, radius: 0, count: 1, refresh: 60 });

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

/** Adds a fetch to the list: by runtime ID, or a row of the same template within a metre (it respawned). */
function merge(rows: CollectedRow[], fetched: Awaited<ReturnType<typeof gameNearby>>): CollectedRow[] {
  const next = rows.map((row) => ({ ...row }));
  const byId = new Map(next.map((row) => [row.runtimeId, row]));
  for (const entity of fetched.rows) {
    if (entity.class === "item") continue;
    const update = { runtimeId: entity.runtimeId, class: entity.class, template: entity.template, label: entity.label, position: entity.position, direction: entity.direction, rotation: entity.rotation, phase: entity.phase };
    const known = byId.get(entity.runtimeId) ?? next.find((row) => row.template === entity.template && row.class === entity.class && distance(row.position, entity.position) < 1);
    if (known) {
      // Spawns that stand still keep their first position; wandering monsters keep where they were first seen.
      Object.assign(known, { ...update, position: known.position, direction: known.direction ?? entity.direction, seen: known.seen + 1 });
      byId.set(entity.runtimeId, known);
    } else {
      const row: CollectedRow = { key: `${entity.runtimeId}:${entity.template}`, ...update, seen: 1 };
      next.push(row);
      byId.set(entity.runtimeId, row);
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
    const list = rows.filter(importable).map((row): NearbyImport => ({ kind: row.class as NearbyImport["kind"], template: row.template, position: row.position, direction: row.direction, rotation: row.rotation, phase: row.phase }));
    if (!list.length) return;
    if (await onImport(list, { count: state.count, refresh: state.refresh })) toggle(rows.map((row) => row.key), false);
  };

  const phaseKnown = state.rows.some((row) => row.phase !== null);
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
    {error && <div className="path-data-message error">{error}</div>}
    {!elementsOpen && <div className="path-data-message">Open the server's elements.data to see names and to tell NPCs from monsters; until then they can be collected but not imported.</div>}
    {elementsOpen && unknown && <div className="path-data-message">Some IDs are not in the open elements.data (orange); they cannot be imported as NPCs or monsters.</div>}
    <div className="npcgen-nearby-table">
      {state.rows.length ? <table className="dyn-table">
        <thead><tr>
          <th><input type="checkbox" checked={allSelected} disabled={!newShown.length} title="Select every new row shown" onChange={(event) => toggle(newShown.map((entry) => entry.row.key), event.target.checked)} /></th>
          <th>Kind</th><th>ID</th><th>Name</th><th>X</th><th>Y</th><th>Z</th><th title="Compass degrees, 0 = north">Facing</th><th title="From where you stood at the last fetch">Distance</th>
          <th title={phaseKnown ? "Phase (0: everyone sees it)" : "Found once phased NPCs or mines are in view"}>Phase</th><th>Status</th><th />
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
