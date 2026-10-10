import { forwardRef, Fragment, useCallback, useDeferredValue, useEffect, useImperativeHandle, useMemo, useRef, useState, type ReactNode } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { ChevronDown, CircleAlert, Copy, Crosshair, Download, FolderOpen, Info, Loader2, Map as MapIcon, Plus, Redo2, RefreshCw, Save, Search, Trash2, TriangleAlert, Undo2, Users, X } from "lucide-react";
import { clientMaps, cloneNpcGenItem, deleteNpcGenItem, dynTaskLabels, compareNpcGen, copyNpcGen, exportNpcGenJson, gameClients, gamePosition, importNpcGenNearby, midmapUrl, npcGenProblems, getNpcGenItem, npcGenView, openNpcGen, pickEssence, redoNpcGen, saveNpcGen, setNpcGenItem, undoNpcGen } from "../elements/api";
import { bytes, count } from "../elements/format";
import { formatDuration } from "../elements/time";
import type { ClientMap, GamePosition, GenComparison, NearbyImport, NpcGenProblem, RunningClient, NpcGenArea, NpcGenController, NpcGenGenerator, NpcGenItem, NpcGenObject, NpcGenResource, NpcGenResourceArea, NpcGenSection, NpcGenSummary, NpcGenTime, NpcGenVec3, NpcGenView } from "../elements/types";
import { NumberInput, TextInput, VertInput } from "./DynTasksEditor";
import { NpcGenCompare } from "./NpcGenCompare";
import { CLASS_COLORS, emptyNearby, NearbyPanel, type NearbyState } from "./NpcGenNearby";
import { ValuePicker } from "./ValuePicker";

export interface NpcGenEditorState {
  loaded: boolean;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  path: string | null;
  version: number | null;
}

export interface NpcGenEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
  save: () => void;
  saveAs: () => void;
  undo: () => void;
  redo: () => void;
  cloneSelected: () => void;
  deleteSelected: () => void;
  toggleMap: () => void;
  /** Tools › Nearby fetch: the window that collects what the game client shows. */
  openNearby: () => void;
  /** Tools › Check problems. */
  openProblems: () => void;
  /** Tools › Compare with another npcgen.data. */
  openCompare: () => void;
  /** Tools › Controller overview. */
  openControllers: () => void;
  /** Tools › Export JSON / Import JSON (the import opens in the comparison). */
  exportJson: () => void;
  importJson: () => void;
}

interface Props {
  active: boolean;
  onStateChange: (state: NpcGenEditorState) => void;
  icon?: (pathId?: number | null) => string | undefined;
  /** The open elements.data, for NPC, monster and mine names. */
  elementsPath?: string | null;
  /** Changes when the client folder changes; null without one (no map images). */
  mapGeneration?: number | null;
}

type PickKind = "npc" | "mine";
const SECTIONS: { key: NpcGenSection; label: string; one: string }[] = [
  { key: "areas", label: "Spawns", one: "spawn area" },
  { key: "resources", label: "Resources", one: "resource area" },
  { key: "objects", label: "Objects", one: "dynamic object" },
  { key: "controllers", label: "Controllers", one: "controller" },
];
// Value names from the official editor (ZElementEditor SceneAIGenerator.cpp, NpcPropertyDlg.cpp) and the server (npcgenerator.cpp).
const NPC_TYPES = ["Monster", "NPC", "Interaction"];
const PLACEMENTS = ["Follows the terrain", "Fixed height (fly)"];
const GROUP_TYPES = ["Normal", "Group", "Boss"];
const REVIVE_TYPES = ["No revive", "Revives after it disappears", "Revives when switched on"];
const AGGRESSIVE = ["As its template", "Aggressive", "Passive"];
const PATH_TYPES = ["Stop at the end", "Back along the path", "Loop"];
const SPEEDS = ["Walk", "Run"];
/** The server adds this to every respawn time (config.h BASE_REBORN_TIME). */
const BASE_RESPAWN = 15;
const U8 = 255, I32_MIN = -2147483648, I32_MAX = 2147483647, F = 1e9;
const BACKUP_KEY = "jdide.npcgen.backup";
const MAP_KEY = "jdide.npcgen.map";
/** The map chosen per file path (instance ID, −1 = none); files without an entry use the detected map. */
const MAP_FOR_KEY = "jdide.npcgen.mapFor";
const BACKGROUND_KEY = "jdide.npcgen.background";

function readStored<T extends object>(key: string, fallback: T): T {
  try {
    const text = localStorage.getItem(key);
    return text ? { ...fallback, ...JSON.parse(text) } : fallback;
  } catch {
    return fallback;
  }
}

function store(key: string, value: unknown) {
  try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* optional */ }
}

/**
 * The map of an npcgen.data: from `npcgen_<map>.data`, else its folder (servers keep one per map folder,
 * named by the instance's data path, e.g. `e12` for Foxhill, whose images are `z12`).
 */
function detectMap(file: string, maps: ClientMap[]): ClientMap | null {
  const parts = file.split(/[\\/]/);
  const stem = /^npcgen[_-](.+)\.data$/i.exec(parts[parts.length - 1] ?? "")?.[1];
  for (const key of [stem, parts[parts.length - 2]]) {
    if (!key) continue;
    const lower = key.toLowerCase();
    const rank = (map: ClientMap) => (map.dataPath.toLowerCase() === lower ? 0 : 2) + (map.hasImage ? 0 : 1);
    const found = maps.filter((map) => map.dataPath.toLowerCase() === lower || map.path.toLowerCase() === lower).sort((a, b) => rank(a) - rank(b));
    if (found.length) return found[0];
  }
  return null;
}
const newGenerator = (): NpcGenGenerator => ({ id: 0, count: 1, refresh: 30, diedTimes: 0, aggressive: 0, offsetWater: 0, offsetTerrain: 0, faction: 0, factionHelper: 0, factionAccept: 0, needHelp: 0, defaultFaction: 1, defaultFactionHelper: 1, defaultFactionAccept: 1, pathId: 0, loopType: 0, speedFlag: 0, deadTime: 0 });
const newResource = (): NpcGenResource => ({ kind: 0, template: 0, refresh: 30, count: 1, heightOffset: 0 });
const noTime = (): NpcGenTime => ({ year: -1, month: -1, week: -1, day: -1, hours: -1, minutes: 0 });
const shortName = (label?: string) => label?.split(" › ").pop();
const capitalize = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/** A titled group of label/value rows. */
function Card({ title, hint, wide, children }: { title: string; hint?: string; wide?: boolean; children: ReactNode }) {
  return <section className={"dyn-section npcgen-card" + (wide ? " wide" : "")}>
    <header><b>{title}</b>{hint && <span className="muted small">{hint}</span>}</header>
    <div className="npcgen-rows">{children}</div>
  </section>;
}

function Row({ label, hint, note, children }: { label: string; hint?: string; note?: ReactNode; children: ReactNode }) {
  return <>
    <span className="npcgen-label" title={hint}>{label}{hint && <span className="npcgen-hint">?</span>}</span>
    <span className="npcgen-value">{children}{note !== undefined && note !== null && note !== false && <span className="muted small">{note}</span>}</span>
  </>;
}

/** A select over named values; a stored value without a name stays selectable. */
function Choice({ value, names, onCommit, disabled }: { value: number; names: string[]; onCommit: (value: number) => void; disabled?: boolean }) {
  return <select className="task-form-select" value={value} disabled={disabled} onChange={(event) => onCommit(Number(event.target.value))}>
    {names.map((name, index) => <option key={index} value={index}>{index} · {name}</option>)}
    {(value < 0 || value >= names.length) && <option value={value}>{value} · unknown</option>}
  </select>;
}

/** A checkbox for a stored byte (any non-zero is on). */
function Flag({ label, value, onCommit, disabled, hint }: { label: string; value: number; onCommit: (value: number) => void; disabled?: boolean; hint?: string }) {
  return <label className="npcgen-flag" title={hint}><input type="checkbox" checked={value !== 0} disabled={disabled} onChange={(event) => onCommit(event.target.checked ? 1 : 0)} /> {label}</label>;
}

/** A field the file's version does not store. */
function Gate({ since, version, children }: { since: number; version: number; children: ReactNode }) {
  return version >= since ? <>{children}</> : <span className="muted small" title={`Stored from npcgen.data version ${since}; this file is version ${version}`}>not in v{version}</span>;
}

function TimeInput({ value, onCommit }: { value: NpcGenTime; onCommit: (value: NpcGenTime) => void }) {
  const parts: [keyof NpcGenTime, string][] = [["year", "year (−1 any)"], ["month", "month 0–11 (−1 any)"], ["week", "weekday 0–6 (−1 any)"], ["day", "day 1–31 (−1 any)"], ["hours", "hour 0–23 (−1 any)"], ["minutes", "minute 0–59"]];
  return <span className="npcgen-time">{parts.map(([part, title]) => <NumberInput key={part} min={I32_MIN} max={I32_MAX} value={value[part]} title={title} placeholder={part} onCommit={(number) => onCommit({ ...value, [part]: number })} />)}</span>;
}

function IdChips({ ids, onCommit, placeholder }: { ids: number[]; onCommit: (ids: number[]) => void; placeholder: string }) {
  const [draft, setDraft] = useState("");
  const add = () => {
    const value = Number(draft.trim());
    if (!Number.isInteger(value)) return;
    setDraft("");
    onCommit([...ids, value]);
  };
  return <div className="dyn-id-list">
    {ids.map((id, index) => <span className="dyn-id-chip" key={`${id}:${index}`}><span className="mono">{id}</span><button className="icon-btn small" onClick={() => onCommit(ids.filter((_, position) => position !== index))}><X size={11} /></button></span>)}
    <span className="dyn-id-add"><input className="dyn-input mono" value={draft} placeholder={placeholder} onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") add(); }} /><button className="btn small" onClick={add} disabled={!draft.trim()}><Plus size={12} /></button></span>
  </div>;
}

/** Facing as a compass angle: 0° = +Z (north on the map), 90° = +X. Keeps the vertical part of the direction. */
/** A unit facing on the ground plane, from the character's direction (spawn areas store this vector). */
function facingVector(position: GamePosition): NpcGenVec3 | null {
  const direction = position.direction;
  const length = direction ? Math.hypot(direction.x, direction.z) : 0;
  return direction && length > 1e-3 ? { x: Math.fround(direction.x / length), y: 0, z: Math.fround(direction.z / length) } : null;
}

/**
 * Resource areas and objects store a rotation as an axis (a3d_CompressDir: two 1/256-turn angles, 0, 0 =
 * upright) and a turn of rad / 255 × 2π (the client's CECMatter). An upright turn of θ faces (sin θ, cos θ).
 */
function facingRotation(position: GamePosition): { direction: [number, number]; radius: number } | null {
  const facing = facingVector(position);
  if (!facing) return null;
  const turn = (Math.atan2(facing.x, facing.z) / (2 * Math.PI) + 1) % 1;
  return { direction: [0, 0], radius: Math.round(turn * 255) % 255 };
}

function facingNote(direction: [number, number], radius: number) {
  if (direction[0] === 0 && direction[1] === 0) return `facing ${Math.round((radius / 255) * 360) % 360}°`;
  return radius === 0 ? "not turned" : "tilted axis";
}

function FacingInput({ value, onCommit }: { value: NpcGenVec3; onCommit: (value: NpcGenVec3) => void }) {
  const flat = Math.hypot(value.x, value.z);
  const degrees = flat > 1e-6 ? Math.round(((Math.atan2(value.x, value.z) * 180) / Math.PI + 360) % 360) : 0;
  return <NumberInput min={-360} max={360} value={degrees} title="Facing in degrees: 0 = north (+Z), 90 = east (+X)" onCommit={(angle) => {
    const radians = (angle * Math.PI) / 180, length = flat > 1e-6 ? flat : Math.sqrt(Math.max(0, 1 - value.y * value.y)) || 1;
    onCommit({ x: Math.fround(Math.sin(radians) * length), y: value.y, z: Math.fround(Math.cos(radians) * length) });
  }} />;
}

// ── Forms ──

interface FormProps<T> {
  value: T;
  version: number;
  labels: Record<string, string>;
  elementsOpen: boolean;
  controllers: NpcGenSummary[];
  onCommit: (value: T, label: string) => void;
  pick: (kind: PickKind, current: number) => Promise<number | null>;
  /** The character's position in a running game client; null when it could not be read (the editor shows why). */
  fromGame: (choose: boolean) => Promise<GamePosition | null>;
  /** Controllers: the areas and objects that name this one. */
  usedBy?: ControllerUse[];
  onShow?: (section: NpcGenSection, index: number) => void;
}

/** An area, resource area or object naming a controller. */
interface ControllerUse {
  section: NpcGenSection;
  index: number;
  text: string;
}

function UsedBy({ uses, onShow }: { uses: ControllerUse[]; onShow?: (section: NpcGenSection, index: number) => void }) {
  if (!uses.length) return <span className="muted small">Nothing uses it.</span>;
  return <span className="npcgen-uses">{uses.map((use) => <button key={`${use.section}:${use.index}`} className="link" onClick={() => onShow?.(use.section, use.index)}>{use.text}</button>)}</span>;
}

/** Takes the position of the character in the running game client. */
function FromGame({ read, onPosition, label = "From game" }: { read: (choose: boolean) => Promise<GamePosition | null>; onPosition: (position: GamePosition) => void; label?: string }) {
  const [busy, setBusy] = useState(false);
  return <button className="btn small" disabled={busy} title={label === "From game" ? "Put it where your character stands, facing the same way, in the running game client (Shift+click: choose the client when several run)" : "Turn it the way your character faces in the running game client (Shift+click: choose the client when several run)"}
    onClick={(event) => {
      setBusy(true);
      void read(event.shiftKey).then((position) => { if (position) onPosition(position); }).finally(() => setBusy(false));
    }}>
    {busy ? <Loader2 size={12} className="spin" /> : <Crosshair size={12} />} {label}
  </button>;
}

/** Areas, resource areas and objects name a controller by its `id` (not its trigger ID). */
function ControllerSelect({ value, controllers, onCommit }: { value: number; controllers: NpcGenSummary[]; onCommit: (value: number) => void }) {
  const known = controllers.some((entry) => entry.ids[0] === value);
  return <select className="task-form-select npcgen-wide-select" value={value} onChange={(event) => onCommit(Number(event.target.value))}>
    <option value={0}>0 · None (always on)</option>
    {controllers.map((entry) => <option key={entry.index} value={entry.ids[0]}>{entry.ids[0]} · {entry.label || "(no name)"}{entry.controller ? ` (trigger ${entry.controller})` : ""}</option>)}
    {value !== 0 && !known && <option value={value}>{value} · no such controller</option>}
  </select>;
}

function NameLabel({ id, labels, elementsOpen }: { id: number; labels: Record<string, string>; elementsOpen: boolean }) {
  const label = labels[String(id)];
  if (label) return <span className="dyn-label" title={label}>{shortName(label)}</span>;
  return elementsOpen && id ? <span className="npcgen-missing">not in elements.data</span> : null;
}

/** Export ID, attached areas and phase, shared by spawn and resource areas. */
function Linking({ version, exportId, attachNum, attached, phase, onCommit }: { version: number; exportId: number; attachNum: number; attached: number[]; phase: number; onCommit: (change: { exportId?: number; attachNum?: number; attached?: number[]; phase?: number }, label: string) => void }) {
  return <>
    <Row label="Export ID" hint="This area's ID in the map; other areas attach to it by this ID"><Gate since={12} version={version}><NumberInput min={I32_MIN} max={I32_MAX} value={exportId} onCommit={(next) => onCommit({ exportId: next }, "Edit export ID")} /></Gate></Row>
    <Row label="Attached" hint="Areas whose regions this area also spawns into (by export ID). An area that is itself attached stores −1 and spawns nothing on its own."><Gate since={12} version={version}>
      <span className="npcgen-stack">
        <label className="npcgen-flag"><input type="checkbox" checked={attachNum < 0} onChange={(event) => onCommit({ attachNum: event.target.checked ? -1 : 0, attached: [] }, "Edit attachment")} /> This area is attached to another</label>
        {attachNum >= 0 && <IdChips ids={attached} placeholder="export ID" onCommit={(next) => onCommit({ attached: next, attachNum: next.length }, "Edit attached areas")} />}
      </span>
    </Gate></Row>
    <Row label="Phase" hint="0: everyone sees it. Otherwise only players in this phase see what spawns here. Some older tools call it Buff Region."><Gate since={14} version={version}><NumberInput min={I32_MIN} max={I32_MAX} value={phase} onCommit={(next) => onCommit({ phase: next }, "Edit phase")} /></Gate></Row>
  </>;
}

function GeneratorDetail({ generator, onCommit }: { generator: NpcGenGenerator; onCommit: (change: Partial<NpcGenGenerator>) => void }) {
  const override = (flag: "defaultFaction" | "defaultFactionHelper" | "defaultFactionAccept", value: "faction" | "factionHelper" | "factionAccept", label: string, hint: string) =>
    <Row label={label} hint={hint}><span className="npcgen-inline">
      <label className="npcgen-flag"><input type="checkbox" checked={generator[flag] === 0} onChange={(event) => onCommit({ [flag]: event.target.checked ? 0 : 1 })} /> Override</label>
      <NumberInput value={generator[value]} title="Faction bits" onCommit={(next) => onCommit({ [value]: next })} />
      {generator[flag] !== 0 && <span className="muted small">template's value is used</span>}
    </span></Row>;
  return <div className="npcgen-detail">
    <Card title="Spawning">
      <Row label="Count" hint="How many spawn from this row"><NumberInput value={generator.count} onCommit={(count) => onCommit({ count })} /></Row>
      <Row label="Respawn (s)" hint={`The server waits ${BASE_RESPAWN} s plus this (at least 15 s, at most 30 days)`} note={`${formatDuration(Math.min(2592000, Math.max(15, BASE_RESPAWN + generator.refresh)))} in game`}><NumberInput value={generator.refresh} onCommit={(refresh) => onCommit({ refresh })} /></Row>
      <Row label="Corpse stays (s)" hint="0: the default. Otherwise the server keeps the body 5–1800 s" note={generator.deadTime ? undefined : "default"}><NumberInput min={I32_MIN} max={I32_MAX} value={generator.deadTime} onCommit={(deadTime) => onCommit({ deadTime })} /></Row>
      <Row label="Death count" hint="Stored by the official editor (default 50); the server does not read it"><NumberInput value={generator.diedTimes} onCommit={(diedTimes) => onCommit({ diedTimes })} /></Row>
    </Card>
    <Card title="Behaviour">
      <Row label="Aggressive"><Choice value={generator.aggressive} names={AGGRESSIVE} onCommit={(aggressive) => onCommit({ aggressive })} /></Row>
      <Row label="Path ID" hint="Patrol path (0: none)"><NumberInput min={I32_MIN} max={I32_MAX} value={generator.pathId} onCommit={(pathId) => onCommit({ pathId })} /></Row>
      <Row label="Path type"><Choice value={generator.loopType} names={PATH_TYPES} disabled={!generator.pathId && generator.loopType === 0} onCommit={(loopType) => onCommit({ loopType })} /></Row>
      <Row label="Speed" hint="Along the patrol path"><Choice value={generator.speedFlag} names={SPEEDS} onCommit={(speedFlag) => onCommit({ speedFlag })} /></Row>
      <Row label="Terrain offset" hint="Height above the ground"><NumberInput float min={-F} max={F} value={generator.offsetTerrain} onCommit={(offsetTerrain) => onCommit({ offsetTerrain })} /></Row>
      <Row label="Water offset" hint="Stored, but the server does not read it"><NumberInput float min={-F} max={F} value={generator.offsetWater} onCommit={(offsetWater) => onCommit({ offsetWater })} /></Row>
    </Card>
    <Card title="Factions" hint="Leave unticked to use the monster template's values">
      {override("defaultFaction", "faction", "Faction", "Which side the monster is on")}
      {override("defaultFactionHelper", "factionHelper", "Asks for help", "Factions it calls for help")}
      {override("defaultFactionAccept", "factionAccept", "Helps", "Factions whose calls it answers")}
      <Row label="Needs help" hint="Stored, but the server does not read it"><Flag label="" value={generator.needHelp} onCommit={(needHelp) => onCommit({ needHelp })} /></Row>
    </Card>
  </div>;
}

function AreaForm({ value: area, version, labels, elementsOpen, controllers, onCommit, pick, fromGame }: FormProps<NpcGenArea>) {
  const [current, setCurrent] = useState(0);
  const set = (change: Partial<NpcGenArea>, label: string) => onCommit({ ...area, ...change }, label);
  const setGen = (index: number, change: Partial<NpcGenGenerator>, label = "Edit generator") => set({ generators: area.generators.map((generator, position) => position === index ? { ...generator, ...change } : generator) }, label);
  const row = Math.min(current, area.generators.length - 1);
  const generator = row >= 0 ? area.generators[row] : null;
  const monster = area.npcType !== 1 && area.npcType !== 2;
  return <div className="dyn-form npcgen-form">
    <div className="npcgen-cards">
      <Card title="General">
        <Row label="Type" hint="Monster, server NPC or interaction object (the server's mob, server and mobactive spawners)"><Choice value={area.npcType} names={NPC_TYPES} onCommit={(npcType) => set({ npcType }, "Edit spawn type")} /></Row>
        <Row label="Placement" hint="Follows the terrain, or keeps the stored height (planes, boxes and flying spawns)"><Choice value={area.kind} names={PLACEMENTS} onCommit={(kind) => set({ kind }, "Edit placement")} /></Row>
        <Row label="Group type" hint="Monsters only: normal monsters, a group that spawns together, or a boss group"><Choice value={area.groupType} names={GROUP_TYPES} disabled={!monster && area.groupType === 0} onCommit={(groupType) => set({ groupType }, "Edit group type")} /></Row>
        <Row label="Controller" hint="The controller that switches this area on and off (0: always on)"><Gate since={7} version={version}><ControllerSelect value={area.controller} controllers={controllers} onCommit={(controller) => set({ controller }, "Edit controller")} /></Gate></Row>
        <Row label="Life time (s)" hint="How long what spawns here lives (0: no limit)" note={version >= 7 && (area.lifeTime > 0 ? formatDuration(area.lifeTime) : "no limit")}><Gate since={7} version={version}><NumberInput min={I32_MIN} max={I32_MAX} value={area.lifeTime} onCommit={(lifeTime) => set({ lifeTime }, "Edit life time")} /></Gate></Row>
        <Linking version={version} exportId={area.exportId} attachNum={area.attachNum} attached={area.attached} phase={area.phase} onCommit={(change, label) => set(change, label)} />
      </Card>
      <Card title="Spawning">
        <Row label="Revive" hint="An NPC area cannot revive when switched on (the official editor refuses it)"><Choice value={area.revive} names={REVIVE_TYPES} onCommit={(revive) => set({ revive }, "Edit revive")} /></Row>
        <Row label="Max count" hint="How many may spawn in total (0: no limit)"><Gate since={7} version={version}><NumberInput min={I32_MIN} max={I32_MAX} value={area.maxCount} onCommit={(maxCount) => set({ maxCount }, "Edit max count")} /></Gate></Row>
        <Row label="Generator ID"><NumberInput value={area.genId} onCommit={(genId) => set({ genId }, "Edit generator ID")} /></Row>
        <Row label="Options"><span className="npcgen-stack">
          <Flag label="Spawn at start" value={area.initGen} onCommit={(initGen) => set({ initGen }, "Edit spawn at start")} />
          <Flag label="Valid once" hint="Spawns only once" value={area.validOnce} onCommit={(validOnce) => set({ validOnce }, "Edit valid once")} />
        </span></Row>
      </Card>
    </div>
    <Card title="Location" hint="Size is the full width, height and depth of the spawn area; 0 is a single point" wide>
      <Row label="Position"><VertInput value={area.position} onCommit={(position) => set({ position }, "Move area")} /><FromGame read={fromGame} onPosition={(found) => { const direction = facingVector(found); set({ position: { x: found.x, y: found.y, z: found.z }, ...(direction ? { direction } : {}) }, "Move area to the character"); }} /></Row>
      <Row label="Direction" note={<>facing <FacingInput value={area.direction} onCommit={(direction) => set({ direction }, "Turn area")} /> °</>}><VertInput value={area.direction} onCommit={(direction) => set({ direction }, "Turn area")} /><FromGame read={fromGame} label="Facing" onPosition={(found) => { const direction = facingVector(found); if (direction) set({ direction }, "Turn area like the character"); }} /></Row>
      <Row label="Size"><VertInput value={area.extents} onCommit={(extents) => set({ extents }, "Resize area")} /></Row>
    </Card>
    <section className="dyn-section"><header><b>Generators</b><span className="muted small">What spawns here · {area.generators.length} row{area.generators.length === 1 ? "" : "s"}, {count(area.generators.reduce((total, entry) => total + entry.count, 0))} spawned</span></header>
      <div className="dyn-section-body"><div className="dyn-table-wrap"><table className="dyn-table npcgen-generators">
        <thead><tr><th>NPC or monster</th><th>Count</th><th>Respawn (s)</th><th>Aggressive</th><th>Path</th><th /></tr></thead>
        <tbody>{area.generators.map((entry, index) => <tr key={index} className={index === row ? "selected" : undefined} onClick={() => setCurrent(index)}>
          <td><span className="dyn-item-cell"><NumberInput value={entry.id} onCommit={(id) => setGen(index, { id })} /><button className="icon-btn small" title="Choose from elements.data" onClick={() => void pick("npc", entry.id).then((id) => { if (id !== null && id !== entry.id) setGen(index, { id }); })}><Search size={12} /></button><NameLabel id={entry.id} labels={labels} elementsOpen={elementsOpen} /></span></td>
          <td><NumberInput value={entry.count} onCommit={(value) => setGen(index, { count: value })} /></td>
          <td><NumberInput value={entry.refresh} onCommit={(refresh) => setGen(index, { refresh })} /></td>
          <td className="muted small">{AGGRESSIVE[entry.aggressive] ?? entry.aggressive}</td>
          <td className="muted small">{entry.pathId ? `${entry.pathId} · ${PATH_TYPES[entry.loopType] ?? entry.loopType}` : "—"}</td>
          <td><span className="task-form-row-actions">
            <button className="icon-btn small" title="Copy this row" onClick={(event) => { event.stopPropagation(); set({ generators: [...area.generators.slice(0, index + 1), { ...entry }, ...area.generators.slice(index + 1)] }, "Copy generator"); setCurrent(index + 1); }}><Copy size={12} /></button>
            <button className="icon-btn small danger" title="Remove this row" onClick={(event) => { event.stopPropagation(); set({ generators: area.generators.filter((_, position) => position !== index) }, "Remove generator"); }}><Trash2 size={12} /></button>
          </span></td>
        </tr>)}</tbody>
      </table></div>
      <div className="dyn-table-foot"><button className="btn small" onClick={() => void pick("npc", 0).then((id) => { if (id) { set({ generators: [...area.generators, { ...newGenerator(), id }] }, "Add generator"); setCurrent(area.generators.length); } })}><Plus size={12} /> Add NPC or monster…</button></div>
      {generator && <>
        <div className="npcgen-detail-title">Row {row + 1}: <span className="mono">{generator.id}</span> <NameLabel id={generator.id} labels={labels} elementsOpen={elementsOpen} /></div>
        <GeneratorDetail generator={generator} onCommit={(change) => setGen(row, change)} />
      </>}
      </div>
    </section>
  </div>;
}

function ResourceForm({ value: area, version, labels, elementsOpen, controllers, onCommit, pick, fromGame }: FormProps<NpcGenResourceArea>) {
  const set = (change: Partial<NpcGenResourceArea>, label: string) => onCommit({ ...area, ...change }, label);
  const setRes = (index: number, change: Partial<NpcGenResource>, label = "Edit resource") => set({ resources: area.resources.map((resource, position) => position === index ? { ...resource, ...change } : resource) }, label);
  return <div className="dyn-form npcgen-form">
    <div className="npcgen-cards">
      <Card title="General">
        <Row label="Controller" hint="The controller that switches this area on and off (0: always on)"><Gate since={7} version={version}><ControllerSelect value={area.controller} controllers={controllers} onCommit={(controller) => set({ controller }, "Edit controller")} /></Gate></Row>
        <Linking version={version} exportId={area.exportId} attachNum={area.attachNum} attached={area.attached} phase={area.phase} onCommit={(change, label) => set(change, label)} />
      </Card>
      <Card title="Spawning">
        <Row label="Max count" hint="0: no limit"><Gate since={7} version={version}><NumberInput min={I32_MIN} max={I32_MAX} value={area.maxCount} onCommit={(maxCount) => set({ maxCount }, "Edit max count")} /></Gate></Row>
        <Row label="Generator ID"><NumberInput value={area.genId} onCommit={(genId) => set({ genId }, "Edit generator ID")} /></Row>
        <Row label="Options"><span className="npcgen-stack">
          <Flag label="Spawn at start" value={area.initGen} onCommit={(initGen) => set({ initGen }, "Edit spawn at start")} />
          <Flag label="Auto revive" value={area.autoRevive} onCommit={(autoRevive) => set({ autoRevive }, "Edit auto revive")} />
          <Flag label="Valid once" value={area.validOnce} onCommit={(validOnce) => set({ validOnce }, "Edit valid once")} />
        </span></Row>
      </Card>
    </div>
    <Card title="Location" hint="Size is the full width and depth" wide>
      <Row label="Position"><VertInput value={area.position} onCommit={(position) => set({ position }, "Move resource area")} /><FromGame read={fromGame} onPosition={(found) => set({ position: { x: found.x, y: found.y, z: found.z }, ...(version >= 6 ? facingRotation(found) ?? {} : {}) }, "Move resource area to the character")} /></Row>
      <Row label="Size X / Z"><span className="dyn-vert"><NumberInput float min={-F} max={F} value={area.extentX} onCommit={(extentX) => set({ extentX }, "Resize resource area")} /><NumberInput float min={-F} max={F} value={area.extentZ} onCommit={(extentZ) => set({ extentZ }, "Resize resource area")} /></span></Row>
      {version >= 6 ? <Row label="Rotation" hint="Axis (two angles) and turn (1/255 of a full turn), as the official editor stores them. Axis 0, 0 is upright; the turn is then the facing (0° = north)." note={facingNote(area.direction, area.radius)}><span className="dyn-vert">{[0, 1].map((part) => <NumberInput key={part} max={U8} value={area.direction[part]} title={part === 0 ? "Axis direction (1/256 turns)" : "Axis tilt from upright (1/256 turns)"} onCommit={(value) => set({ direction: part === 0 ? [value, area.direction[1]] : [area.direction[0], value] }, "Edit rotation")} />)}<NumberInput max={U8} value={area.radius} title="Turn about the axis (1/255 turns)" onCommit={(radius) => set({ radius }, "Edit rotation")} /></span><FromGame read={fromGame} label="Facing" onPosition={(found) => { const facing = facingRotation(found); if (facing) set(facing, "Turn resource area like the character"); }} /></Row> : <Row label="Rotation"><Gate since={6} version={version}>{null}</Gate></Row>}
    </Card>
    <section className="dyn-section"><header><b>Resources</b><span className="muted small">Mines and herbs</span></header>
      <div className="dyn-section-body"><div className="dyn-table-wrap"><table className="dyn-table">
        <thead><tr><th>Mine</th><th>Type</th><th>Count</th><th title={`The server adds ${BASE_RESPAWN} s`}>Respawn (s)</th><th>Height offset</th><th /></tr></thead>
        <tbody>{area.resources.map((resource, index) => <tr key={index}>
          <td><span className="dyn-item-cell"><NumberInput min={I32_MIN} max={I32_MAX} value={resource.template} onCommit={(template) => setRes(index, { template })} /><button className="icon-btn small" title="Choose from elements.data" onClick={() => void pick("mine", resource.template).then((template) => { if (template !== null && template !== resource.template) setRes(index, { template }); })}><Search size={12} /></button><NameLabel id={resource.template} labels={labels} elementsOpen={elementsOpen} /></span></td>
          <td><NumberInput min={I32_MIN} max={I32_MAX} value={resource.kind} onCommit={(kind) => setRes(index, { kind })} /></td>
          <td><NumberInput value={resource.count} onCommit={(value) => setRes(index, { count: value })} /></td>
          <td><NumberInput value={resource.refresh} onCommit={(refresh) => setRes(index, { refresh })} /></td>
          <td><NumberInput float min={-F} max={F} value={resource.heightOffset} onCommit={(heightOffset) => setRes(index, { heightOffset })} /></td>
          <td><button className="icon-btn small danger" onClick={() => set({ resources: area.resources.filter((_, position) => position !== index) }, "Remove resource")}><Trash2 size={12} /></button></td>
        </tr>)}</tbody>
      </table></div>
      <div className="dyn-table-foot"><button className="btn small" onClick={() => void pick("mine", 0).then((template) => { if (template) set({ resources: [...area.resources, { ...newResource(), template }] }, "Add resource"); })}><Plus size={12} /> Add mine…</button></div>
      </div>
    </section>
  </div>;
}

function ObjectForm({ value: object, version, controllers, onCommit, fromGame }: FormProps<NpcGenObject>) {
  const set = (change: Partial<NpcGenObject>, label: string) => onCommit({ ...object, ...change }, label);
  return <div className="dyn-form npcgen-form"><div className="npcgen-cards">
    <Card title="General">
      <Row label="Object ID" hint="dynamicobjects.data entry"><NumberInput value={object.id} onCommit={(id) => set({ id }, "Edit object ID")} /></Row>
      <Row label="Controller" hint="The controller that shows and hides it (0: always shown)"><Gate since={10} version={version}><ControllerSelect value={object.controller} controllers={controllers} onCommit={(controller) => set({ controller }, "Edit controller")} /></Gate></Row>
      <Row label="Phase" hint="0: everyone sees it; otherwise only players in this phase"><Gate since={14} version={version}><NumberInput min={I32_MIN} max={I32_MAX} value={object.phase} onCommit={(phase) => set({ phase }, "Edit phase")} /></Gate></Row>
    </Card>
    <Card title="Location">
      <Row label="Position"><VertInput value={object.position} onCommit={(position) => set({ position }, "Move object")} /><FromGame read={fromGame} onPosition={(found) => set({ position: { x: found.x, y: found.y, z: found.z }, ...(facingRotation(found) ?? {}) }, "Move object to the character")} /></Row>
      <Row label="Rotation" hint="Axis (two angles) and turn (1/255 of a full turn), as the official editor stores them. Axis 0, 0 is upright; the turn is then the facing (0° = north)." note={facingNote(object.direction, object.radius)}><span className="dyn-vert">{[0, 1].map((part) => <NumberInput key={part} max={U8} value={object.direction[part]} title={part === 0 ? "Axis direction (1/256 turns)" : "Axis tilt from upright (1/256 turns)"} onCommit={(value) => set({ direction: part === 0 ? [value, object.direction[1]] : [object.direction[0], value] }, "Edit rotation")} />)}<NumberInput max={U8} value={object.radius} title="Turn about the axis (1/255 turns)" onCommit={(radius) => set({ radius }, "Edit rotation")} /></span><FromGame read={fromGame} label="Facing" onPosition={(found) => { const facing = facingRotation(found); if (facing) set(facing, "Turn object like the character"); }} /></Row>
      <Row label="Scale" hint="16 = 1.0" note={version >= 9 && `${(object.scale / 16).toFixed(2)}×`}><Gate since={9} version={version}><NumberInput max={U8} value={object.scale} onCommit={(scale) => set({ scale }, "Edit scale")} /></Gate></Row>
    </Card>
  </div></div>;
}

function ControllerForm({ value: controller, version, onCommit, usedBy = [], onShow }: FormProps<NpcGenController>) {
  const set = (change: Partial<NpcGenController>, label: string) => onCommit({ ...controller, ...change }, label);
  return <div className="dyn-form npcgen-form">
    <div className="npcgen-cards">
      <Card title="General">
        <Row label="ID" hint="Areas and objects name their controller by this ID"><NumberInput value={controller.id} onCommit={(id) => set({ id }, "Edit controller ID")} /></Row>
        <Row label="Trigger ID" hint="The ID GM commands and scripts use to switch it"><NumberInput min={I32_MIN} max={I32_MAX} value={controller.controllerId} onCommit={(controllerId) => set({ controllerId }, "Edit trigger ID")} /></Row>
        <Row label="Name" hint="At most 127 GBK bytes"><TextInput value={controller.name} onCommit={(name) => set({ name }, "Rename controller")} /></Row>
        <Row label="Options"><span className="npcgen-stack">
          <Flag label="Active at start" value={controller.active} onCommit={(active) => set({ active }, "Edit active")} />
          {version >= 11 && <Flag label="Repeats" value={controller.repeat} onCommit={(repeat) => set({ repeat }, "Edit repeat")} />}
        </span></Row>
      </Card>
      <Card title="Timing">
        <Row label="Wait time (s)" hint="Before it switches on"><NumberInput min={I32_MIN} max={I32_MAX} value={controller.waitTime} onCommit={(waitTime) => set({ waitTime }, "Edit wait time")} /></Row>
        <Row label="Stop time (s)" hint="How long it stays on"><NumberInput min={I32_MIN} max={I32_MAX} value={controller.stopTime} onCommit={(stopTime) => set({ stopTime }, "Edit stop time")} /></Row>
        <Row label="Active time range"><Gate since={8} version={version}><NumberInput min={I32_MIN} max={I32_MAX} value={controller.activeTimeRange} onCommit={(activeTimeRange) => set({ activeTimeRange }, "Edit active range")} /></Gate></Row>
      </Card>
    </div>
    <Card title="Used by" hint="What this controller switches on and off" wide>
      <Row label={`${usedBy.length} item${usedBy.length === 1 ? "" : "s"}`}><UsedBy uses={usedBy} onShow={onShow} /></Row>
    </Card>
    <Card title="Dates" hint="year · month · weekday · day · hour · minute; −1 = any" wide>
      <Row label="Starts at"><span className="npcgen-inline"><TimeInput value={controller.activeTime} onCommit={(activeTime) => set({ activeTime }, "Edit start time")} /><Flag label="Ignore" value={controller.activeTimeInvalid} onCommit={(activeTimeInvalid) => set({ activeTimeInvalid }, "Edit start time")} /></span></Row>
      <Row label="Stops at"><span className="npcgen-inline"><TimeInput value={controller.stopTimeAt} onCommit={(stopTimeAt) => set({ stopTimeAt }, "Edit stop time")} /><Flag label="Ignore" value={controller.stopTimeInvalid} onCommit={(stopTimeInvalid) => set({ stopTimeInvalid }, "Edit stop time")} /></span></Row>
    </Card>
    <section className="dyn-section"><header><b>Time segments</b><span className="muted small">When it is on (version 13+)</span>{version >= 13 && <label className="npcgen-flag small">Logic <NumberInput min={0} max={32767} value={controller.segmentLogic >> 16} title="Stored in the high bits of the segment count" onCommit={(logic) => set({ segmentLogic: logic << 16 }, "Edit segment logic")} /></label>}</header>
      {version >= 13 ? <div className="dyn-section-body">
        {controller.segments.map(([start, end], index) => <div className="npcgen-segment" key={index}>
          <TimeInput value={start} onCommit={(next) => set({ segments: controller.segments.map((segment, position) => position === index ? [next, segment[1]] : segment) }, "Edit time segment")} />
          <span className="muted">to</span>
          <TimeInput value={end} onCommit={(next) => set({ segments: controller.segments.map((segment, position) => position === index ? [segment[0], next] : segment) }, "Edit time segment")} />
          <button className="icon-btn small danger" onClick={() => set({ segments: controller.segments.filter((_, position) => position !== index) }, "Remove time segment")}><Trash2 size={12} /></button>
        </div>)}
        <div className="dyn-table-foot"><button className="btn small" onClick={() => set({ segments: [...controller.segments, [noTime(), noTime()]] }, "Add time segment")}><Plus size={12} /> Add segment</button></div>
      </div> : <div className="empty-note">Not stored in version {version}.</div>}
    </section>
  </div>;
}

// ── Map plot ──

const mapLabel = (map: ClientMap) => `${map.name} (${map.dataPath || map.path})`;

/** A map list with a search box: name, server folder, image name or ID; arrow keys and Enter choose. */
function MapChooser({ maps, value, detected, onChoose }: { maps: ClientMap[]; value: ClientMap | null; detected: ClientMap | null; onChoose: (id: number) => void }) {
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [cursor, setCursor] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDown = (event: MouseEvent) => { if (!root.current?.contains(event.target as Node)) setOpen(false); };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);
  const needle = text.trim().toLowerCase();
  // None and the detected map first; while searching, exact folder, image or ID matches first.
  const options = useMemo<(ClientMap | null)[]>(() => {
    const sorted = [...maps].sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }) || a.id - b.id);
    if (!needle) return [null, ...(detected ? [detected] : []), ...sorted.filter((map) => map !== detected)];
    const exact = (map: ClientMap) => map.dataPath.toLowerCase() === needle || map.path.toLowerCase() === needle || String(map.id) === needle;
    const found = sorted.filter((map) => [map.name, map.dataPath, map.path, String(map.id)].some((part) => part.toLowerCase().includes(needle)));
    return [...found.filter(exact), ...found.filter((map) => !exact(map))];
  }, [detected, maps, needle]);
  useEffect(() => { (list.current?.children[cursor] as HTMLElement | undefined)?.scrollIntoView({ block: "nearest" }); }, [cursor, open]);
  const choose = (map: ClientMap | null) => {
    onChoose(map?.id ?? -1);
    setOpen(false);
  };
  const toggle = () => {
    setText("");
    setCursor(0);
    setOpen((current) => !current);
  };
  return <div className="npcgen-map-chooser" ref={root}>
    <button className="btn small npcgen-map-button" onClick={toggle} title={value ? `Map ${value.id}: server folder ${value.dataPath}, images ${value.path}` : "No map image"}>
      <span className="truncate">{value ? mapLabel(value) : "None"}</span><ChevronDown size={12} />
    </button>
    {open && <div className="npcgen-map-menu">
      <div className="npcgen-map-search"><Search size={13} /><input autoFocus value={text} placeholder="Map name, folder or ID" onChange={(event) => { setText(event.target.value); setCursor(0); }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") { event.preventDefault(); setCursor((current) => Math.min(options.length - 1, current + 1)); }
          else if (event.key === "ArrowUp") { event.preventDefault(); setCursor((current) => Math.max(0, current - 1)); }
          else if (event.key === "Enter" && options.length) { event.preventDefault(); choose(options[Math.min(cursor, options.length - 1)]); }
          else if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setOpen(false); }
        }} /></div>
      <div className="npcgen-map-options" role="listbox" ref={list}>
        {options.map((entry, index) => <button key={entry ? `${entry.id}:${index}` : "none"} role="option" aria-selected={(entry?.id ?? -1) === (value?.id ?? -1)}
          className={"npcgen-map-option" + (index === cursor ? " cursor" : "") + ((entry?.id ?? -1) === (value?.id ?? -1) ? " selected" : "")}
          onMouseEnter={() => setCursor(index)} onClick={() => choose(entry)}>
          {entry ? <>
            <span className="truncate">{entry.name}{entry.id === detected?.id && <span className="tag ok">detected</span>}{!entry.hasImage && <span className="muted small"> · no image</span>}</span>
            <span className="mono muted small">{entry.dataPath || entry.path}</span>
            <span className="mono muted small">{entry.id}</span>
          </> : <span className="muted">None (no map image)</span>}
        </button>)}
        {!options.length && <div className="empty-note">No map matches.</div>}
      </div>
    </div>}
  </div>;
}

const COLORS = { areas: ["#d9534f", "#3b82c4", "#9b59b6"], resources: "#2e9e5b", objects: "#888" };

/** The client's midmap of the map: it spans ±rows × 512 on both axes, north up (CDlgMidMap). */
interface Background {
  url: string;
  half: number;
  opacity: number;
}

/** Something collected from the game, drawn as a diamond. */
interface Marker {
  key: string;
  x: number;
  z: number;
  color: string;
  title: string;
  active: boolean;
  /** Groups: the area they cover. */
  width?: number;
  depth?: number;
}

function MapPlot({ view, section, selected, onSelect, background, controls, onImageError, markers }: { view: NpcGenView; section: NpcGenSection | null; selected: number | null; onSelect: (section: NpcGenSection, index: number) => void; background: Background | null; controls?: ReactNode; onImageError?: () => void; markers?: Marker[] }) {
  const points = useMemo(() => [...view.areas, ...view.resources, ...view.objects], [view]);
  const bounds = useMemo(() => {
    if (background) return { x: -background.half, y: -background.half, w: background.half * 2, h: background.half * 2 };
    if (!points.length) return { x: -512, y: -512, w: 1024, h: 1024 };
    let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity;
    for (const point of points) {
      minX = Math.min(minX, point.x - point.extX / 2); maxX = Math.max(maxX, point.x + point.extX / 2);
      minZ = Math.min(minZ, point.z - point.extZ / 2); maxZ = Math.max(maxZ, point.z + point.extZ / 2);
    }
    const pad = Math.max(20, (maxX - minX + maxZ - minZ) * 0.03);
    return { x: minX - pad, y: -maxZ - pad, w: maxX - minX + pad * 2, h: maxZ - minZ + pad * 2 };
  }, [background, points]);
  const [box, setBox] = useState(bounds);
  // Fit when another file or map is shown, not after every edit.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => setBox(bounds), [view.path, background?.half, !!background]);
  const svg = useRef<SVGSVGElement>(null);
  const drag = useRef<{ x: number; y: number; box: typeof box } | null>(null);
  const toWorld = (clientX: number, clientY: number, current = box) => {
    const rect = svg.current!.getBoundingClientRect();
    const scale = Math.max(current.w / rect.width, current.h / rect.height);
    const offsetX = (rect.width * scale - current.w) / 2, offsetY = (rect.height * scale - current.h) / 2;
    return { x: current.x - offsetX + (clientX - rect.left) * scale, y: current.y - offsetY + (clientY - rect.top) * scale, scale };
  };
  const unit = box.w / 600;
  const shape = (point: NpcGenSummary, kind: NpcGenSection, color: string) => {
    const active = kind === section && point.index === selected;
    const common = { onClick: (event: React.MouseEvent) => { event.stopPropagation(); onSelect(kind, point.index); }, className: "npcgen-shape" + (active ? " active" : "") };
    // Extents are full sizes (the server's rect is position ± extent / 2).
    return point.extX > 1 || point.extZ > 1
      ? <rect key={`${kind}:${point.index}`} {...common} x={point.x - point.extX / 2} y={-point.z - point.extZ / 2} width={point.extX} height={point.extZ} fill={color} fillOpacity={active ? 0.45 : 0.18} stroke={active ? "var(--accent)" : color} strokeWidth={active ? 3 : 1} vectorEffect="non-scaling-stroke"><title>{SECTIONS.find((entry) => entry.key === kind)?.one} {point.index + 1}</title></rect>
      : <circle key={`${kind}:${point.index}`} {...common} cx={point.x} cy={-point.z} r={unit * (active ? 6 : 3)} fill={active ? "var(--accent)" : color}><title>{SECTIONS.find((entry) => entry.key === kind)?.one} {point.index + 1}</title></circle>;
  };
  return <div className="npcgen-map">
    {controls && <div className="npcgen-mapbar">{controls}</div>}
    <svg ref={svg} viewBox={`${box.x} ${box.y} ${box.w} ${box.h}`} preserveAspectRatio="xMidYMid meet"
      onWheel={(event) => { const at = toWorld(event.clientX, event.clientY); const factor = event.deltaY > 0 ? 1.2 : 1 / 1.2; setBox((current) => ({ x: at.x - (at.x - current.x) * factor, y: at.y - (at.y - current.y) * factor, w: current.w * factor, h: current.h * factor })); }}
      onMouseDown={(event) => { drag.current = { x: event.clientX, y: event.clientY, box }; }}
      onMouseMove={(event) => { const start = drag.current; if (!start || !svg.current) return; const scale = toWorld(0, 0, start.box).scale; setBox({ ...start.box, x: start.box.x - (event.clientX - start.x) * scale, y: start.box.y - (event.clientY - start.y) * scale }); }}
      onMouseUp={() => { drag.current = null; }} onMouseLeave={() => { drag.current = null; }} onDoubleClick={() => setBox(bounds)}>
      {background && <image href={background.url} x={-background.half} y={-background.half} width={background.half * 2} height={background.half * 2} opacity={background.opacity} preserveAspectRatio="none" pointerEvents="none" onError={onImageError} />}
      {!background && <>
        <line x1={box.x} x2={box.x + box.w} y1={0} y2={0} className="npcgen-axis" vectorEffect="non-scaling-stroke" />
        <line y1={box.y} y2={box.y + box.h} x1={0} x2={0} className="npcgen-axis" vectorEffect="non-scaling-stroke" />
      </>}
      {view.resources.map((point) => shape(point, "resources", COLORS.resources))}
      {view.objects.map((point) => shape(point, "objects", COLORS.objects))}
      {view.areas.map((point) => shape(point, "areas", COLORS.areas[point.kind] ?? "#c08a2e"))}
      {markers?.map((marker) => {
        const size = unit * (marker.active ? 7 : 4);
        if (marker.width && marker.depth) {
          return <rect key={marker.key} className="npcgen-marker" x={marker.x - marker.width / 2} y={-marker.z - marker.depth / 2} width={marker.width} height={marker.depth} fill={marker.color} fillOpacity={0.15} stroke={marker.active ? "var(--accent)" : marker.color} strokeDasharray="4 3" strokeWidth={marker.active ? 2.5 : 1.5} vectorEffect="non-scaling-stroke"><title>{marker.title}</title></rect>;
        }
        return <rect key={marker.key} className="npcgen-marker" x={marker.x - size / 2} y={-marker.z - size / 2} width={size} height={size} transform={`rotate(45 ${marker.x} ${-marker.z})`} fill={marker.color} stroke={marker.active ? "var(--accent)" : "#fff"} strokeWidth={marker.active ? 2.5 : 1} vectorEffect="non-scaling-stroke"><title>{marker.title}</title></rect>;
      })}
    </svg>
    <div className="npcgen-legend"><span><i style={{ background: COLORS.areas[0] }} />Monsters</span><span><i style={{ background: COLORS.areas[1] }} />NPCs</span><span><i style={{ background: COLORS.areas[2] }} />Interaction</span><span><i style={{ background: COLORS.resources }} />Resources</span><span><i style={{ background: COLORS.objects }} />Objects</span>{markers?.length ? <span><i className="npcgen-legend-diamond" />Collected</span> : null}<span className="muted">Wheel: zoom · drag: move · double-click: fit · north is up</span></div>
  </div>;
}

// ── Controller overview ──

function ControllerOverview({ view, uses, onShow, onClose }: { view: NpcGenView; uses: Map<number, ControllerUse[]>; onShow: (section: NpcGenSection, index: number) => void; onClose: () => void }) {
  const [filter, setFilter] = useState<"all" | "used" | "unused">("all");
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<Set<number>>(new Set());
  const needle = query.trim().toLowerCase();
  const known = new Set(view.controllers.map((entry) => entry.ids[0]));
  const missing = [...uses.entries()].filter(([id]) => !known.has(id)).sort((a, b) => a[0] - b[0]);
  const rows = view.controllers.filter((entry) => {
    const used = (uses.get(entry.ids[0])?.length ?? 0) > 0;
    if ((filter === "used" && !used) || (filter === "unused" && used)) return false;
    return !needle || entry.label.toLowerCase().includes(needle) || String(entry.ids[0]) === needle || String(entry.controller) === needle;
  });
  const tally = (list: ControllerUse[] | undefined) => {
    const parts = (["areas", "resources", "objects"] as const).map((kind) => [kind, list?.filter((use) => use.section === kind).length ?? 0] as const).filter(([, n]) => n);
    return parts.length ? parts.map(([kind, n]) => `${n} ${kind === "areas" ? "spawn" : kind === "resources" ? "resource" : "object"}${n === 1 ? "" : "s"}`).join(", ") : "unused";
  };
  const toggle = (id: number) => setOpen((current) => { const next = new Set(current); if (next.has(id)) next.delete(id); else next.add(id); return next; });
  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal npcgen-nearby-dialog" role="dialog" aria-label="Controller overview" onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head"><h3>Controller overview</h3><span className="muted small">{count(view.controllers.length)} controllers · what each switches on and off</span><span className="spacer" /><button className="icon-btn" onClick={onClose} aria-label="Close" title="Close (Esc)"><X size={16} /></button></header>
      <div className="npcgen-nearby">
        <div className="npcgen-nearby-bar">
          <span className="dyn-compare-tabs">{(["all", "used", "unused"] as const).map((entry) => <button key={entry} className={"btn small" + (filter === entry ? " active" : "")} onClick={() => setFilter(entry)}>{entry === "all" ? "All" : entry === "used" ? "Used" : "Unused"}</button>)}</span>
          <span className="dyn-task-search npcgen-overview-search"><Search size={13} /><input value={query} placeholder="Name, ID or trigger ID" onChange={(event) => setQuery(event.target.value)} /></span>
        </div>
        <div className="npcgen-nearby-table">
          {missing.length > 0 && <div className="path-data-message error">{missing.map(([id, list]) => <div key={id}>Controller {id} does not exist but is used by: <UsedBy uses={list} onShow={onShow} /></div>)}</div>}
          <table className="dyn-table">
            <thead><tr><th>Controller</th><th>ID</th><th>Trigger</th><th>Name</th><th>At start</th><th>Used by</th></tr></thead>
            <tbody>{rows.map((entry) => {
              const id = entry.ids[0];
              const list = uses.get(id) ?? [];
              return <Fragment key={entry.index}>
                <tr onClick={() => list.length && toggle(id)} className={list.length ? undefined : "npcgen-nearby-known"}>
                  <td><button className="link" onClick={(event) => { event.stopPropagation(); onShow("controllers", entry.index); }}>Controller {entry.index + 1}</button></td>
                  <td className="mono">{id}</td>
                  <td className="mono">{entry.controller || ""}</td>
                  <td className="truncate">{entry.label || <span className="muted">(no name)</span>}</td>
                  <td>{entry.kind ? <span className="tag ok">on</span> : <span className="muted small">off</span>}</td>
                  <td>{list.length ? <span>{open.has(id) ? "▾" : "▸"} {tally(list)}</span> : <span className="muted small">unused</span>}</td>
                </tr>
                {open.has(id) && <tr className="npcgen-overview-uses"><td colSpan={6}><UsedBy uses={list} onShow={onShow} /></td></tr>}
              </Fragment>;
            })}</tbody>
          </table>
          {!rows.length && <div className="empty-note">No controllers match.</div>}
        </div>
      </div>
    </div>
  </div>;
}

// ── The workspace ──

export const NpcGenEditor = forwardRef<NpcGenEditorHandle, Props>(function NpcGenEditor({ active, onStateChange, icon, elementsPath = null, mapGeneration = null }, ref) {
  const [view, setView] = useState<NpcGenView | null>(null);
  const [section, setSection] = useState<NpcGenSection>("areas");
  /** The Nearby fetch window (Tools menu); what it collected stays while it is closed. */
  const [nearbyOpen, setNearbyOpen] = useState(false);
  /** The problems window: the last check (null while checking) and whether notes show. */
  const [problemsOpen, setProblemsOpen] = useState(false);
  const [compareOpen, setCompareOpen] = useState(false);
  const [comparison, setComparison] = useState<GenComparison | null>(null);
  const [controllersOpen, setControllersOpen] = useState(false);
  /** The export window: what to export and whether what it uses comes along. */
  const [exporting, setExporting] = useState<{ scope: "file" | "selected" | "shown"; related: boolean } | null>(null);
  const [problems, setProblems] = useState<NpcGenProblem[] | null>(null);
  const [showNotes, setShowNotes] = useState(false);
  /** Half the size of the detected map (rows × 512), for the outside-the-map check. */
  const halfSize = useRef<number | null>(null);
  const checkProblems = useCallback(async () => {
    setProblems(null);
    try {
      setProblems(await npcGenProblems(halfSize.current));
    } catch (problem) {
      setProblemsOpen(false);
      setError(String(problem).replace(/^Error: /, ""));
    }
  }, []);
  const openProblems = useCallback(() => {
    setProblemsOpen(true);
    void checkProblems();
  }, [checkProblems]);
  const [nearby, setNearby] = useState<NearbyState>(emptyNearby);
  const [focusedRow, setFocusedRow] = useState<string | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [item, setItem] = useState<NpcGenItem | null>(null);
  const [query, setQuery] = useState("");
  const needle = useDeferredValue(query.trim().toLowerCase());
  const [labels, setLabels] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [saving, setSaving] = useState<{ target: string | null; changed: boolean } | null>(null);
  const [backup, setBackup] = useState(() => { try { return localStorage.getItem(BACKUP_KEY) !== "0"; } catch { return true; } });
  const [showMap, setShowMap] = useState(() => { try { return localStorage.getItem(MAP_KEY) !== "0"; } catch { return true; } });
  const [picking, setPicking] = useState<{ kind: PickKind; current: number; resolve: (value: number | null) => void } | null>(null);
  const savedOnce = useRef(false);

  useEffect(() => { npcGenView().then((current) => { if (current) setView(current); }).catch(() => {}); }, []);
  useEffect(() => onStateChange({ loaded: !!view, dirty: !!view?.dirty, canUndo: !!view?.canUndo, canRedo: !!view?.canRedo, path: view?.path ?? null, version: view?.version ?? null }), [onStateChange, view]);

  // Names of every spawned template (once per file and elements.data).
  useEffect(() => setLabels({}), [elementsPath]);

  // The client's maps, for the plot's background.
  const [maps, setMaps] = useState<ClientMap[] | null>(null);
  const [mapsError, setMapsError] = useState<string | null>(null);
  const [mapFor, setMapFor] = useState<Record<string, number>>(() => readStored(MAP_FOR_KEY, {}));
  const [backgroundStyle, setBackgroundStyle] = useState(() => readStored(BACKGROUND_KEY, { on: true, opacity: 70 }));
  const [failedImage, setFailedImage] = useState<string | null>(null);
  useEffect(() => {
    setMaps(null);
    setMapsError(null);
    if (mapGeneration === null) return;
    let cancelled = false;
    clientMaps().then((found) => { if (!cancelled) setMaps(found); }).catch((problem) => { if (!cancelled) setMapsError(String(problem).replace(/^Error: /, "")); });
    return () => { cancelled = true; };
  }, [mapGeneration]);
  useEffect(() => {
    if (!view || !elementsPath) return;
    const compared = (comparison?.rows ?? []).filter((row) => row.section === "areas" || row.section === "resources").flatMap((row) => row.ids);
    const ids = [...new Set([...view.areas, ...view.resources].flatMap((entry) => entry.ids).concat(compared))].filter((id) => id && !(String(id) in labels));
    if (ids.length) dynTaskLabels(ids, []).then((found) => setLabels((current) => ({ ...current, ...found.elements }))).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view, elementsPath, comparison]);

  /** Who names each controller ID (areas, resource areas, objects). */
  const controllerUses = useMemo(() => {
    const uses = new Map<number, ControllerUse[]>();
    if (!view) return uses;
    for (const [kind, entries] of [["areas", view.areas], ["resources", view.resources], ["objects", view.objects]] as const) {
      for (const entry of entries) {
        if (!entry.controller) continue;
        const first = entry.ids[0];
        const name = kind === "objects" ? `object ${first}` : first ? shortName(labels[String(first)]) ?? `#${first}` : "(empty)";
        const text = `${capitalize(SECTIONS.find((section) => section.key === kind)!.one)} ${entry.index + 1} · ${name}`;
        uses.set(entry.controller, [...(uses.get(entry.controller) ?? []), { section: kind, index: entry.index, text }]);
      }
    }
    return uses;
  }, [labels, view]);

  useEffect(() => {
    if (!view || selected === null) { setItem(null); return; }
    let cancelled = false;
    getNpcGenItem(section, selected).then((next) => { if (!cancelled) setItem(next); }).catch(() => { if (!cancelled) setItem(null); });
    return () => { cancelled = true; };
  }, [section, selected, view]);

  const run = useCallback(async (work: () => Promise<NpcGenView | void>) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const next = await work();
      if (next) setView(next);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  }, [busy]);

  const load = useCallback(async (path: string) => {
    if (view?.dirty && !window.confirm("npcgen.data has unsaved changes. Open another file and discard them?")) return;
    await run(async () => {
      const next = await openNpcGen(path);
      setSelected(null);
      setLabels({});
      savedOnce.current = false;
      return next;
    });
  }, [run, view?.dirty]);
  const choose = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, defaultPath: view?.path, title: "Open a map's npcgen.data (server config folder)", filters: [{ name: "npcgen.data", extensions: ["data"] }] });
    if (typeof picked === "string") await load(picked);
  }, [load, view?.path]);

  const commit = useCallback((next: NpcGenItem, label: string) => {
    if (selected === null) return;
    void run(() => setNpcGenItem(selected, next, label));
  }, [run, selected]);
  const pick = useCallback((kind: PickKind, current: number) => new Promise<number | null>((resolve) => setPicking({ kind, current, resolve })), []);

  // The character's position in a running game client; with several clients, the one chosen last.
  const [choosingClient, setChoosingClient] = useState<{ clients: RunningClient[]; resolve: (pid: number | null) => void } | null>(null);
  const lastClient = useRef<number | null>(null);
  /** The client to read: the only one, the one chosen last, or ask (`choose` asks again); null when cancelled. */
  const pickClient = useCallback(async (choose: boolean): Promise<number | null> => {
    const clients = await gameClients();
    if (!clients.length) throw new Error("No game client is running. Start elementclient.exe and enter the world first.");
    let pid = clients.length === 1 ? clients[0].pid : choose ? undefined : clients.find((client) => client.pid === lastClient.current)?.pid;
    if (pid === undefined) {
      const chosen = await new Promise<number | null>((resolve) => setChoosingClient({ clients, resolve }));
      setChoosingClient(null);
      if (chosen === null) return null;
      pid = chosen;
    }
    lastClient.current = pid;
    return pid;
  }, []);
  const fromGame = useCallback(async (choose: boolean): Promise<GamePosition | null> => {
    setError(null);
    setNote(null);
    try {
      const pid = await pickClient(choose);
      return pid === null ? null : await gamePosition(pid);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
      return null;
    }
  }, [pickClient]);
  const importNearby = useCallback(async (rows: NearbyImport[], options: { count: number; refresh: number }) => {
    let added = false;
    await run(async () => {
      const next = await importNpcGenNearby(rows, options);
      added = true;
      return next;
    });
    if (added) setNote(`Added ${rows.length === 1 ? "1 item" : `${rows.length} items`} from the game (one undo step). Save to keep them.`);
    return added;
  }, [run]);
  const pickSearch = useMemo(() => picking ? (text: string, page: number) => pickEssence(picking.kind, text, page, picking.current || null) : undefined, [picking]);

  const undo = useCallback(() => { if (view?.canUndo) void run(undoNpcGen); }, [run, view?.canUndo]);
  const redo = useCallback(() => { if (view?.canRedo) void run(redoNpcGen); }, [run, view?.canRedo]);
  const openCompare = useCallback(() => {
    setCompareOpen(true);
    // Edits since the last comparison change it: compare again.
    if (comparison) compareNpcGen(null).then(setComparison).catch(() => setComparison(null));
  }, [comparison]);
  const importJson = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, title: "Import npcgen JSON", filters: [{ name: "npcgen JSON export", extensions: ["json"] }] });
    if (typeof picked !== "string") return;
    let opened = false;
    await run(async () => {
      setComparison(await compareNpcGen(picked));
      opened = true;
    });
    if (opened) setCompareOpen(true);
  }, [run]);
  const chooseCompare = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, defaultPath: comparison?.path ?? view?.path, title: "Compare with another npcgen.data or a JSON export", filters: [{ name: "npcgen.data or JSON export", extensions: ["data", "json"] }] });
    if (typeof picked !== "string") return;
    await run(async () => { setComparison(await compareNpcGen(picked)); });
  }, [comparison?.path, run, view?.path]);
  const copyCompared = useCallback(async (picks: [NpcGenSection, number][]) => {
    let report: Awaited<ReturnType<typeof copyNpcGen>>["report"] | null = null;
    await run(async () => {
      const result = await copyNpcGen(picks);
      report = result.report;
      setComparison(await compareNpcGen(null));
      return result.view;
    });
    const done = report as Awaited<ReturnType<typeof copyNpcGen>>["report"] | null;
    if (!done) return;
    const parts = [done.added && `copied ${done.added}`, done.replaced && `replaced ${done.replaced}`, done.controllers && `also copied ${done.controllers} controller${done.controllers === 1 ? "" : "s"} they use`, done.droppedAttachments && `left out ${done.droppedAttachments} attachment${done.droppedAttachments === 1 ? "" : "s"} to areas this file lacks`, done.fitted && `cleared values version ${view?.version} cannot store in ${done.fitted}`].filter(Boolean);
    setNote(`${parts.join(", ").replace(/^./, (first) => first.toUpperCase())} (one undo step). Save to keep them.`);
  }, [run, view?.version]);
  const showItem = useCallback((section: NpcGenSection, index: number) => {
    setCompareOpen(false);
    setControllersOpen(false);
    setSection(section);
    setSelected(index);
  }, []);

  const cloneSelected = useCallback(() => {
    if (selected === null || nearbyOpen || compareOpen || controllersOpen || problemsOpen) return;
    void run(async () => { const result = await cloneNpcGenItem(section, selected); setSelected(result.index); return result.view; });
  }, [run, section, selected, nearbyOpen, compareOpen, controllersOpen, problemsOpen]);
  const deleteSelected = useCallback(() => {
    if (selected === null || nearbyOpen || compareOpen || controllersOpen || problemsOpen) return;
    const one = SECTIONS.find((entry) => entry.key === section)!.one;
    if (!window.confirm(`Delete ${one} ${selected + 1}? Undo brings it back.`)) return;
    void run(async () => { const next = await deleteNpcGenItem(section, selected); const total = next[section].length; setSelected(total ? Math.min(selected, total - 1) : null); return next; });
  }, [run, section, selected, nearbyOpen, compareOpen, controllersOpen, problemsOpen]);
  const toggleMap = useCallback(() => setShowMap((current) => { try { localStorage.setItem(MAP_KEY, current ? "0" : "1"); } catch { /* optional */ } return !current; }), []);

  const writeTo = useCallback(async (target: string | null, replaceChanged: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const report = await saveNpcGen(target, backup, replaceChanged);
      savedOnce.current = true;
      setSaving(null);
      setNote(`Saved ${bytes(report.size)} to ${report.path}.${report.backup ? ` Backup: ${report.backup}.` : ""} Restart the map's server to load it.`);
      setView(await npcGenView());
    } catch (problem) {
      const message = String(problem).replace(/^Error: /, "");
      if (message.startsWith("CHANGED_ON_DISK")) setSaving({ target, changed: true });
      else setError(message);
    } finally {
      setBusy(false);
    }
  }, [backup]);
  const saveCurrent = useCallback(() => { if (!view) return; if (savedOnce.current) void writeTo(null, false); else setSaving({ target: null, changed: false }); }, [view, writeTo]);
  const saveAs = useCallback(async () => {
    if (!view) return;
    const target = await save({ defaultPath: view.path, title: "Save npcgen.data as", filters: [{ name: "npcgen.data", extensions: ["data"] }] });
    if (target) setSaving({ target, changed: false });
  }, [view]);

  useImperativeHandle(ref, () => ({ choose: () => void choose(), openPath: (path) => void load(path), save: saveCurrent, saveAs: () => void saveAs(), undo, redo, cloneSelected, deleteSelected, toggleMap, openNearby: () => { if (view) setNearbyOpen(true); }, openProblems: () => { if (view) openProblems(); }, openCompare: () => { if (view) openCompare(); }, openControllers: () => { if (view) setControllersOpen(true); }, exportJson: () => { if (view) setExporting((current) => current ?? { scope: "file", related: true }); }, importJson: () => { if (view) void importJson(); } }), [choose, cloneSelected, deleteSelected, importJson, load, openCompare, openProblems, redo, saveAs, saveCurrent, toggleMap, undo, view]);

  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      const mod = event.ctrlKey || event.metaKey;
      const typing = event.target instanceof HTMLElement && /^(INPUT|TEXTAREA|SELECT)$/.test(event.target.tagName);
      const key = event.key.toLowerCase();
      if (event.key === "Escape" && (nearbyOpen || problemsOpen || compareOpen || controllersOpen) && !choosingClient) {
        event.preventDefault();
        setNearbyOpen(false);
        setProblemsOpen(false);
        setCompareOpen(false);
        setControllersOpen(false);
        return;
      }
      if (mod && event.shiftKey && key === "m") {
        event.preventDefault();
        event.stopImmediatePropagation();
        if (view) openProblems();
        return;
      }
      if (mod && ["o", "s", "z", "y", "d"].includes(key)) {
        if (typing && (key === "z" || key === "y")) return;
        event.preventDefault();
        event.stopImmediatePropagation();
        if (key === "o") void choose();
        else if (key === "s") event.shiftKey ? void saveAs() : saveCurrent();
        else if (key === "z") event.shiftKey ? redo() : undo();
        else if (key === "y") redo();
        else cloneSelected();
      } else if (event.key === "Delete" && !typing) {
        event.preventDefault();
        deleteSelected();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [active, choose, choosingClient, cloneSelected, compareOpen, controllersOpen, deleteSelected, nearbyOpen, openProblems, problemsOpen, redo, saveAs, saveCurrent, undo, view]);

  const rows = useMemo(() => (view?.[section] ?? []).filter((entry) => {
    if (!needle) return true;
    if (String(entry.index + 1) === needle || entry.label.toLowerCase().includes(needle) || String(entry.controller) === needle) return true;
    return entry.ids.some((id) => String(id) === needle || (labels[String(id)] ?? "").toLowerCase().includes(needle));
  }), [labels, needle, section, view]);

  if (!view) return <section className="dyn-tasks-pane empty">
    <div className="drop-card tasks-empty">
      <Users size={34} />
      <h2>Open an npcgen.data file</h2>
      <p className="muted">A map's spawns: monster and NPC areas, mines and herbs, dynamic objects and event controllers. Only the <b>server</b> reads these files, one per map folder (for example <span className="mono">gamed/config/z1/npcgen.data</span>); the copy in a client's data folder is not used.</p>
      <button className="btn primary" onClick={() => void choose()} disabled={busy}>{busy ? <Loader2 size={14} className="spin" /> : <FolderOpen size={14} />} Choose npcgen.data…</button>
      {error && <div className="path-data-message error">{error}</div>}
    </div>
  </section>;

  const detected = maps ? detectMap(view.path, maps) : null;
  const storedMap = mapFor[view.path];
  const map = !maps ? null : storedMap === undefined ? detected : storedMap === -1 ? null : maps.find((entry) => entry.id === storedMap) ?? detected;
  const chooseMap = (id: number) => {
    const next = { ...mapFor };
    if (id === (detected?.id ?? -1)) delete next[view.path];
    else next[view.path] = id;
    setMapFor(next);
    store(MAP_FOR_KEY, next);
  };
  const setStyle = (change: Partial<typeof backgroundStyle>) => setBackgroundStyle((current) => { const next = { ...current, ...change }; store(BACKGROUND_KEY, next); return next; });
  halfSize.current = map && map.rows > 0 ? map.rows * 512 : null;
  const imageUrl = map?.hasImage && map.rows > 0 && mapGeneration !== null ? midmapUrl(mapGeneration, map.path) : null;
  const background = imageUrl && backgroundStyle.on && failedImage !== imageUrl ? { url: imageUrl, half: map!.rows * 512, opacity: backgroundStyle.opacity / 100 } : null;
  const mapControls = mapGeneration === null ? <span className="muted small">Set the game client folder in Settings to show the map image.</span>
    : mapsError ? <span className="muted small" title={mapsError}>Map images are not available: {mapsError}</span>
    : !maps ? <span className="muted small"><Loader2 size={12} className="spin" /> Reading the client's maps…</span>
    : <>
      <span className="npcgen-flag">Map <MapChooser maps={maps} value={map} detected={detected} onChoose={chooseMap} /></span>
      {!detected && storedMap === undefined && <span className="muted small" title="Name the file npcgen_<map>.data or keep it in the server's map folder">not detected</span>}
      <label className="npcgen-flag"><input type="checkbox" checked={backgroundStyle.on} disabled={!imageUrl} onChange={(event) => setStyle({ on: event.target.checked })} /> Map image</label>
      <input type="range" className="npcgen-opacity" min={10} max={100} step={5} value={backgroundStyle.opacity} disabled={!background} title={`Image opacity ${backgroundStyle.opacity}%`} onChange={(event) => setStyle({ opacity: Number(event.target.value) })} />
      {map && !map.hasImage && <span className="muted small">This map has no image in surfaces.pck.</span>}
      {imageUrl && failedImage === imageUrl && <span className="muted small">The map image could not be read.</span>}
    </>;
  const generators = view.areas.reduce((total, entry) => total + entry.ids.length, 0);
  const rowLabel = (entry: NpcGenSummary) => {
    if (section === "controllers") return <><span className="mono">{entry.ids[0]}</span><span className="truncate">{entry.label || <span className="muted">(no name)</span>}{entry.controller !== 0 && <span className="muted small"> · trigger {entry.controller}</span>}</span>{entry.kind !== 0 ? <span className="tag ok">on</span> : <span />}{(() => { const uses = controllerUses.get(entry.ids[0])?.length ?? 0; return <span className={"small " + (uses ? "muted" : "npcgen-unused")} title={uses ? "Areas and objects using it" : "Nothing uses this controller"}>{uses ? `${uses}×` : "unused"}</span>; })()}</>;
    const badge = entry.controller !== 0 ? <span className="dyn-award-badge" title={`Controller ${entry.controller}`}>⚑{entry.controller}</span> : <span />;
    if (section === "objects") return <><span className="mono">{entry.ids[0]}</span><span className="truncate muted">Object</span><span />{badge}</>;
    const first = entry.ids[0];
    const label = first ? labels[String(first)] : undefined;
    const name = !first ? <span className="muted">(empty)</span> : label ? shortName(label) : elementsPath ? <span className="npcgen-missing">not found</span> : null;
    return <>
      {section === "areas" ? <span className={`npcgen-kind k${entry.kind}`} title={NPC_TYPES[entry.kind] ?? `Type ${entry.kind}`}>{NPC_TYPES[entry.kind]?.[0] ?? entry.kind}</span> : <span />}
      <span className="truncate" title={label}><span className="mono">{first || ""}</span> {name}{entry.ids.length > 1 && <span className="muted"> +{entry.ids.length - 1}</span>}</span>
      <span className="muted small">×{entry.count}</span>
      {badge}
    </>;
  };
  const form = item && selected !== null ? (() => {
    const key = `${item.section}:${selected}`;
    const common = { version: view.version, labels, elementsOpen: !!elementsPath, controllers: view.controllers, pick, fromGame };
    switch (item.section) {
      case "areas": return <AreaForm key={key} {...common} value={item.item} onCommit={(value, label) => commit({ section: "areas", item: value }, label)} />;
      case "resources": return <ResourceForm key={key} {...common} value={item.item} onCommit={(value, label) => commit({ section: "resources", item: value }, label)} />;
      case "objects": return <ObjectForm key={key} {...common} value={item.item} onCommit={(value, label) => commit({ section: "objects", item: value }, label)} />;
      default: return <ControllerForm key={key} {...common} value={item.item} usedBy={controllerUses.get(item.item.id) ?? []} onShow={showItem} onCommit={(value, label) => commit({ section: "controllers", item: value }, label)} />;
    }
  })() : null;

  return <section className="dyn-tasks-pane" aria-busy={busy}>
    <header className="tasks-head">
      <div><h2>NPC generator {view.dirty && <span className="tag warn">unsaved</span>}</h2><div className="tasks-file-line">
        <span className="mono truncate" title={view.path}>{view.path}</span>
        <span className="path-data-badge"><b>Version:</b> {view.version}</span>
        {map && <span className="path-data-badge" title={`Map ${map.id}: server folder ${map.dataPath}, images ${map.path}`}><b>Map:</b> {map.name}</span>}
        <span className="path-data-badge"><b>Spawn areas:</b> {count(view.areas.length)}</span>
        <span className="path-data-badge"><b>Generators:</b> {count(generators)}</span>
        <span className="path-data-badge"><b>Size:</b> {bytes(view.size)}</span>
      </div></div>
      <button className="btn" onClick={undo} disabled={!view.canUndo || busy} title="Undo (Ctrl+Z)"><Undo2 size={14} /></button>
      <button className="btn" onClick={redo} disabled={!view.canRedo || busy} title="Redo (Ctrl+Y)"><Redo2 size={14} /></button>
      <button className={"btn" + (showMap ? " active" : "")} onClick={toggleMap} title="Show the map plot"><MapIcon size={14} /> Map</button>
      <button className="btn" onClick={() => void choose()} disabled={busy}><FolderOpen size={14} /> Open…</button>
      <button className="btn primary" onClick={saveCurrent} disabled={busy}><Save size={14} /> Save</button>
    </header>
    {error && <div className="path-data-message error">{error} <button className="link" onClick={() => setError(null)}>Dismiss</button></div>}
    {note && <div className="path-data-message ok">{note} <button className="link" onClick={() => setNote(null)}>Dismiss</button></div>}
    <div className="dyn-tasks-body">
      <aside className="dyn-task-list">
        <div className="npcgen-sections">{SECTIONS.map((entry) => <button key={entry.key} className={"btn small" + (section === entry.key ? " active" : "")} onClick={() => { setSection(entry.key); setSelected(null); }}>{entry.label} <span className="muted">{count(view[entry.key].length)}</span></button>)}</div>
        <div className="dyn-task-search"><Search size={13} /><input value={query} placeholder="NPC, mine, ID, controller or number" onChange={(event) => setQuery(event.target.value)} />{query && <button className="icon-btn small" onClick={() => setQuery("")}><X size={12} /></button>}</div>
        <div className="dyn-task-rows" role="listbox">
          {rows.map((entry) => <button key={entry.index} role="option" aria-selected={entry.index === selected} className={"dyn-task-row npcgen-row" + (entry.index === selected ? " selected" : "")} onClick={() => setSelected(entry.index)}>
            <span className="mono muted">{entry.index + 1}</span>{rowLabel(entry)}{entry.changed && <span className="changed-dot" title="Changed" />}
          </button>)}
          {!rows.length && <div className="empty-note">Nothing matches.</div>}
        </div>
        <footer>
          <button className="btn small" onClick={cloneSelected} disabled={selected === null || busy} title="Copy below (Ctrl+D)"><Copy size={13} /> Clone</button>
          <button className="btn small danger" onClick={deleteSelected} disabled={selected === null || busy} title="Delete (Del)"><Trash2 size={13} /> Delete</button>
          <span className="spacer" />
          <span className="muted small">{count(rows.length)} / {count(view[section].length)}</span>
        </footer>
      </aside>
      <div className={"npcgen-main" + (showMap && section !== "controllers" ? " with-map" : "")}>
        {showMap && section !== "controllers" && <MapPlot view={view} section={section} selected={selected} onSelect={(kind, index) => { setSection(kind); setSelected(index); }} background={background} controls={mapControls} onImageError={() => setFailedImage(imageUrl)}
          markers={nearby.rows.filter((row) => nearby.shown[row.class === "unknown" ? "npc" : row.class as keyof NearbyState["shown"]] ?? true).map((row) => ({ key: row.key, x: row.position.x, z: row.position.z, color: CLASS_COLORS[row.class], title: `Collected: ${row.label ?? row.template} (${row.template})${row.members ? ` × ${row.members.length}` : ""}`, active: row.key === focusedRow, width: row.size?.x, depth: row.size?.z }))} />}
        <div className="dyn-task-form-scroll">
          {selected === null ? <div className="empty-note">Select a {SECTIONS.find((entry) => entry.key === section)!.one}{section !== "controllers" ? " in the list or on the map" : ""}.</div>
            : <><div className="dyn-task-title"><h3>{capitalize(SECTIONS.find((entry) => entry.key === section)!.one)} {selected + 1}</h3></div>{form ?? <div className="empty-note">Loading…</div>}</>}
        </div>
      </div>
    </div>
    {compareOpen && <div className="modal-backdrop" onMouseDown={() => setCompareOpen(false)}>
      <div className="modal npcgen-nearby-dialog" role="dialog" aria-label="Compare" onMouseDown={(event) => event.stopPropagation()}>
        <header className="modal-head"><h3>Compare with another npcgen.data</h3><span className="spacer" /><button className="icon-btn" onClick={() => setCompareOpen(false)} aria-label="Close" title="Close (Esc); the comparison stays"><X size={16} /></button></header>
        <NpcGenCompare comparison={comparison} version={view.version} busy={busy} labels={labels} onChoose={() => void chooseCompare()} onCopy={(picks) => void copyCompared(picks)} onShow={showItem} />
      </div>
    </div>}
    {exporting && (() => {
      const one = SECTIONS.find((entry) => entry.key === section)!;
      const picks: [NpcGenSection, number][] | null = exporting.scope === "file" ? null : exporting.scope === "selected" ? (selected === null ? [] : [[section, selected]]) : rows.map((entry) => [section, entry.index] as [NpcGenSection, number]);
      const folder = view.path.split(/[\\/]/).slice(-2, -1)[0] ?? "npcgen";
      const write = async () => {
        const target = await save({ defaultPath: view.path.replace(/[^\\/]*$/, `${folder}_npcgen${exporting.scope === "file" ? "" : `_${section}`}.json`), title: "Export npcgen JSON", filters: [{ name: "JSON", extensions: ["json"] }] });
        if (!target) return;
        try {
          const counts = await exportNpcGenJson(target, picks, exporting.related);
          const parts = [counts.areas && `${counts.areas} spawn area${counts.areas === 1 ? "" : "s"}`, counts.resources && `${counts.resources} resource area${counts.resources === 1 ? "" : "s"}`, counts.objects && `${counts.objects} object${counts.objects === 1 ? "" : "s"}`, counts.controllers && `${counts.controllers} controller${counts.controllers === 1 ? "" : "s"}`].filter(Boolean);
          setExporting(null);
          setNote(`Exported ${parts.join(", ")} to ${target}.`);
        } catch (problem) {
          setError(String(problem).replace(/^Error: /, ""));
        }
      };
      return <div className="modal-backdrop" onMouseDown={() => setExporting(null)}>
        <div className="modal dyn-save-dialog" role="dialog" aria-label="Export JSON" onMouseDown={(event) => event.stopPropagation()}>
          <h3>Export JSON</h3>
          <div className="npcgen-stack npcgen-export-scope">
            <label className="npcgen-flag"><input type="radio" checked={exporting.scope === "file"} onChange={() => setExporting({ ...exporting, scope: "file" })} /> The whole file <span className="muted small">({count(view.areas.length)} spawn areas, {count(view.resources.length)} resource areas, {count(view.objects.length)} objects, {count(view.controllers.length)} controllers)</span></label>
            <label className="npcgen-flag"><input type="radio" checked={exporting.scope === "selected"} disabled={selected === null} onChange={() => setExporting({ ...exporting, scope: "selected" })} /> The selected item {selected !== null && <span className="muted small">({capitalize(one.one)} {selected + 1})</span>}</label>
            <label className="npcgen-flag"><input type="radio" checked={exporting.scope === "shown"} disabled={!rows.length} onChange={() => setExporting({ ...exporting, scope: "shown" })} /> {one.label} shown in the list <span className="muted small">({count(rows.length)}{query.trim() ? ` matching “${query.trim()}”` : ""})</span></label>
            <label className="npcgen-flag" title="So an import into another file is complete"><input type="checkbox" checked={exporting.related || exporting.scope === "file"} disabled={exporting.scope === "file"} onChange={(event) => setExporting({ ...exporting, related: event.target.checked })} /> Include the controllers and attached areas they use</label>
          </div>
          <p className="muted small">The JSON keeps every stored value and the npcgen.data version. Import it with Tools › Import JSON…: it opens like a comparison, so you choose what to copy.</p>
          <footer><span className="spacer" /><button className="btn" onClick={() => setExporting(null)}>Cancel</button><button className="btn primary" disabled={!!picks && !picks.length} onClick={() => void write()}><Download size={14} /> Export…</button></footer>
        </div>
      </div>;
    })()}
    {controllersOpen && <ControllerOverview view={view} uses={controllerUses} onShow={showItem} onClose={() => setControllersOpen(false)} />}
    {problemsOpen && (() => {
      const rank = { error: 0, warning: 1, note: 2 };
      const order = SECTIONS.map((entry) => entry.key);
      const sorted = [...(problems ?? [])].sort((a, b) => rank[a.severity] - rank[b.severity] || order.indexOf(a.section) - order.indexOf(b.section) || a.index - b.index);
      const tally = (severity: NpcGenProblem["severity"]) => (problems ?? []).filter((problem) => problem.severity === severity).length;
      const shown = sorted.filter((problem) => showNotes || problem.severity !== "note");
      const name = (problem: NpcGenProblem) => {
        const entry = view[problem.section][problem.index];
        if (!entry) return null;
        if (problem.section === "controllers") return entry.label || null;
        const first = entry.ids[0];
        return first ? shortName(labels[String(first)]) ?? `#${first}` : null;
      };
      const icon = { error: <CircleAlert size={14} />, warning: <TriangleAlert size={14} />, note: <Info size={14} /> };
      return <div className="modal-backdrop" onMouseDown={() => setProblemsOpen(false)}>
        <div className="modal npcgen-problems-dialog" role="dialog" aria-label="Problems" onMouseDown={(event) => event.stopPropagation()}>
          <header className="modal-head">
            <h3>Problems</h3>
            {problems && <span className="muted small">{count(tally("error"))} error{tally("error") === 1 ? "" : "s"} · {count(tally("warning"))} warning{tally("warning") === 1 ? "" : "s"}</span>}
            <span className="spacer" />
            <label className="npcgen-flag small" title="Harmless but probably unintended, common in official files (empty areas, unused attachments)"><input type="checkbox" checked={showNotes} onChange={(event) => setShowNotes(event.target.checked)} /> Notes ({count(tally("note"))})</label>
            <button className="btn small" onClick={() => void checkProblems()} disabled={!problems}><RefreshCw size={12} /> Check again</button>
            <button className="icon-btn" onClick={() => setProblemsOpen(false)} aria-label="Close" title="Close (Esc)"><X size={16} /></button>
          </header>
          <div className="npcgen-problems-list">
            {!problems ? <div className="empty-note"><Loader2 size={14} className="spin" /> Checking…</div>
              : shown.length ? shown.map((problem, index) => <button key={index} className={`npcgen-problem ${problem.severity}`} title="Show it" onClick={() => { setProblemsOpen(false); setSection(problem.section); setSelected(problem.index); }}>
                {icon[problem.severity]}
                <span className="npcgen-problem-item">{capitalize(SECTIONS.find((entry) => entry.key === problem.section)!.one)} {problem.index + 1}</span>
                <span className="npcgen-problem-name truncate">{name(problem)}</span>
                <span className="npcgen-problem-message">{problem.message}</span>
              </button>)
              : <div className="empty-note">No problems found{tally("note") && !showNotes ? ` (${count(tally("note"))} notes hidden)` : ""}.</div>}
          </div>
          <footer className="modal-foot muted small">
            {elementsPath ? "Templates are checked against the open elements.data." : "Open the server's elements.data to check NPC, monster and mine templates too."}
            {" "}{halfSize.current ? `Positions are checked against the map (±${halfSize.current}).` : "The map is unknown, so positions are not checked."}
          </footer>
        </div>
      </div>;
    })()}
    {nearbyOpen && <div className="modal-backdrop" onMouseDown={() => setNearbyOpen(false)}>
      <div className="modal npcgen-nearby-dialog" role="dialog" aria-label="Nearby fetch" onMouseDown={(event) => event.stopPropagation()}>
        <header className="modal-head"><h3>Nearby fetch</h3><span className="muted small">What your game client has loaded around your character</span><span className="spacer" /><button className="icon-btn" onClick={() => setNearbyOpen(false)} aria-label="Close" title="Close (Esc); the list stays"><X size={16} /></button></header>
        <NearbyPanel view={view} state={nearby} setState={setNearby} elementsOpen={!!elementsPath} busy={busy} pickClient={pickClient} onImport={importNearby} onShow={(row) => setFocusedRow(row.key)} />
      </div>
    </div>}
    {choosingClient && <div className="modal-backdrop" onMouseDown={() => choosingClient.resolve(null)}>
      <div className="modal dyn-save-dialog" onMouseDown={(event) => event.stopPropagation()}>
        <h3>Which game client?</h3>
        <p className="muted small">Several clients are running. The position comes from the character in the one you choose; later clicks use it again (Shift+click asks again).</p>
        <div className="npcgen-clients">{choosingClient.clients.map((client) => <button key={client.pid} className="btn" onClick={() => choosingClient.resolve(client.pid)}>
          <b>Process {client.pid}</b><span className="mono muted small truncate" title={client.path ?? undefined}>{client.path ?? "elementclient.exe"}</span>
        </button>)}</div>
        <footer><span className="spacer" /><button className="btn" onClick={() => choosingClient.resolve(null)}>Cancel</button></footer>
      </div>
    </div>}
    {picking && pickSearch && <ValuePicker search={pickSearch} icon={icon} onApply={async (value) => { picking.resolve(Number(value)); return null; }} onClose={() => { picking.resolve(null); setPicking(null); }} />}
    {saving && <div className="modal-backdrop" onMouseDown={() => !busy && setSaving(null)}>
      <div className="modal dyn-save-dialog" onMouseDown={(event) => event.stopPropagation()}>
        <h3>Save npcgen.data</h3>
        <p className="mono truncate" title={saving.target ?? view.path}>{saving.target ?? view.path}</p>
        <p className="muted small">The file keeps its version ({view.version}). The server reads it when the map starts, so restart that map's server.</p>
        {saving.changed && <div className="path-data-message error">Another program changed this file since it was opened. Replacing it discards those changes.</div>}
        <label className="small"><input type="checkbox" checked={backup} onChange={(event) => { setBackup(event.target.checked); try { localStorage.setItem(BACKUP_KEY, event.target.checked ? "1" : "0"); } catch { /* optional */ } }} /> Back up the existing file first (a .7z in <span className="mono">jdide_backups</span> next to it, once per session)</label>
        <footer><span className="spacer" /><button className="btn" onClick={() => setSaving(null)} disabled={busy}>Cancel</button><button className="btn primary" onClick={() => void writeTo(saving.target, saving.changed)} disabled={busy}>{busy ? <Loader2 size={14} className="spin" /> : <Save size={14} />} {saving.changed ? "Replace anyway" : "Save"}</button></footer>
      </div>
    </div>}
  </section>;
});
