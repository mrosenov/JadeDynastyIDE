import { forwardRef, useCallback, useDeferredValue, useEffect, useImperativeHandle, useMemo, useRef, useState, type ReactNode } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Copy, FolderOpen, Loader2, Map as MapIcon, Plus, Redo2, Save, Search, Trash2, Undo2, Users, X } from "lucide-react";
import { cloneNpcGenItem, deleteNpcGenItem, dynTaskLabels, getNpcGenItem, npcGenView, openNpcGen, pickEssence, redoNpcGen, saveNpcGen, setNpcGenItem, undoNpcGen } from "../elements/api";
import { bytes, count } from "../elements/format";
import { formatDuration } from "../elements/time";
import type { NpcGenArea, NpcGenController, NpcGenGenerator, NpcGenItem, NpcGenObject, NpcGenResource, NpcGenResourceArea, NpcGenSection, NpcGenSummary, NpcGenTime, NpcGenVec3, NpcGenView } from "../elements/types";
import { NumberInput, TextInput, VertInput } from "./DynTasksEditor";
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
}

interface Props {
  active: boolean;
  onStateChange: (state: NpcGenEditorState) => void;
  icon?: (pathId?: number | null) => string | undefined;
  /** The open elements.data, for NPC, monster and mine names. */
  elementsPath?: string | null;
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
const newGenerator = (): NpcGenGenerator => ({ id: 0, count: 1, refresh: 30, diedTimes: 0, aggressive: 0, offsetWater: 0, offsetTerrain: 0, faction: 0, factionHelper: 0, factionAccept: 0, needHelp: 0, defaultFaction: 1, defaultFactionHelper: 1, defaultFactionAccept: 1, pathId: 0, loopType: 0, speedFlag: 0, deadTime: 0 });
const newResource = (): NpcGenResource => ({ kind: 0, template: 0, refresh: 30, count: 1, heightOffset: 0 });
const noTime = (): NpcGenTime => ({ year: -1, month: -1, week: -1, day: -1, hours: -1, minutes: 0 });
const shortName = (label?: string) => label?.split(" › ").pop();

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

function AreaForm({ value: area, version, labels, elementsOpen, controllers, onCommit, pick }: FormProps<NpcGenArea>) {
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
      <Row label="Position"><VertInput value={area.position} onCommit={(position) => set({ position }, "Move area")} /></Row>
      <Row label="Direction" note={<>facing <FacingInput value={area.direction} onCommit={(direction) => set({ direction }, "Turn area")} /> °</>}><VertInput value={area.direction} onCommit={(direction) => set({ direction }, "Turn area")} /></Row>
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

function ResourceForm({ value: area, version, labels, elementsOpen, controllers, onCommit, pick }: FormProps<NpcGenResourceArea>) {
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
      <Row label="Position"><VertInput value={area.position} onCommit={(position) => set({ position }, "Move resource area")} /></Row>
      <Row label="Size X / Z"><span className="dyn-vert"><NumberInput float min={-F} max={F} value={area.extentX} onCommit={(extentX) => set({ extentX }, "Resize resource area")} /><NumberInput float min={-F} max={F} value={area.extentZ} onCommit={(extentZ) => set({ extentZ }, "Resize resource area")} /></span></Row>
      <Row label="Direction / radius" hint="Two direction bytes and a radius byte, as the official editor stores them"><Gate since={6} version={version}><span className="dyn-vert">{[0, 1].map((part) => <NumberInput key={part} max={U8} value={area.direction[part]} onCommit={(value) => set({ direction: part === 0 ? [value, area.direction[1]] : [area.direction[0], value] }, "Edit direction")} />)}<NumberInput max={U8} value={area.radius} onCommit={(radius) => set({ radius }, "Edit direction")} /></span></Gate></Row>
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

function ObjectForm({ value: object, version, controllers, onCommit }: FormProps<NpcGenObject>) {
  const set = (change: Partial<NpcGenObject>, label: string) => onCommit({ ...object, ...change }, label);
  return <div className="dyn-form npcgen-form"><div className="npcgen-cards">
    <Card title="General">
      <Row label="Object ID" hint="dynamicobjects.data entry"><NumberInput value={object.id} onCommit={(id) => set({ id }, "Edit object ID")} /></Row>
      <Row label="Controller" hint="The controller that shows and hides it (0: always shown)"><Gate since={10} version={version}><ControllerSelect value={object.controller} controllers={controllers} onCommit={(controller) => set({ controller }, "Edit controller")} /></Gate></Row>
      <Row label="Phase" hint="0: everyone sees it; otherwise only players in this phase"><Gate since={14} version={version}><NumberInput min={I32_MIN} max={I32_MAX} value={object.phase} onCommit={(phase) => set({ phase }, "Edit phase")} /></Gate></Row>
    </Card>
    <Card title="Location">
      <Row label="Position"><VertInput value={object.position} onCommit={(position) => set({ position }, "Move object")} /></Row>
      <Row label="Direction / radius"><span className="dyn-vert">{[0, 1].map((part) => <NumberInput key={part} max={U8} value={object.direction[part]} onCommit={(value) => set({ direction: part === 0 ? [value, object.direction[1]] : [object.direction[0], value] }, "Edit direction")} />)}<NumberInput max={U8} value={object.radius} onCommit={(radius) => set({ radius }, "Edit direction")} /></span></Row>
      <Row label="Scale" hint="16 = 1.0" note={version >= 9 && `${(object.scale / 16).toFixed(2)}×`}><Gate since={9} version={version}><NumberInput max={U8} value={object.scale} onCommit={(scale) => set({ scale }, "Edit scale")} /></Gate></Row>
    </Card>
  </div></div>;
}

function ControllerForm({ value: controller, version, onCommit }: FormProps<NpcGenController>) {
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

const COLORS = { areas: ["#d9534f", "#3b82c4", "#9b59b6"], resources: "#2e9e5b", objects: "#888" };

function MapPlot({ view, section, selected, onSelect }: { view: NpcGenView; section: NpcGenSection; selected: number | null; onSelect: (section: NpcGenSection, index: number) => void }) {
  const points = useMemo(() => [...view.areas, ...view.resources, ...view.objects], [view]);
  const bounds = useMemo(() => {
    if (!points.length) return { x: -512, y: -512, w: 1024, h: 1024 };
    let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity;
    for (const point of points) {
      minX = Math.min(minX, point.x - point.extX / 2); maxX = Math.max(maxX, point.x + point.extX / 2);
      minZ = Math.min(minZ, point.z - point.extZ / 2); maxZ = Math.max(maxZ, point.z + point.extZ / 2);
    }
    const pad = Math.max(20, (maxX - minX + maxZ - minZ) * 0.03);
    return { x: minX - pad, y: -maxZ - pad, w: maxX - minX + pad * 2, h: maxZ - minZ + pad * 2 };
  }, [points]);
  const [box, setBox] = useState(bounds);
  useEffect(() => setBox(bounds), [bounds]);
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
    <svg ref={svg} viewBox={`${box.x} ${box.y} ${box.w} ${box.h}`} preserveAspectRatio="xMidYMid meet"
      onWheel={(event) => { const at = toWorld(event.clientX, event.clientY); const factor = event.deltaY > 0 ? 1.2 : 1 / 1.2; setBox((current) => ({ x: at.x - (at.x - current.x) * factor, y: at.y - (at.y - current.y) * factor, w: current.w * factor, h: current.h * factor })); }}
      onMouseDown={(event) => { drag.current = { x: event.clientX, y: event.clientY, box }; }}
      onMouseMove={(event) => { const start = drag.current; if (!start || !svg.current) return; const scale = toWorld(0, 0, start.box).scale; setBox({ ...start.box, x: start.box.x - (event.clientX - start.x) * scale, y: start.box.y - (event.clientY - start.y) * scale }); }}
      onMouseUp={() => { drag.current = null; }} onMouseLeave={() => { drag.current = null; }} onDoubleClick={() => setBox(bounds)}>
      <line x1={box.x} x2={box.x + box.w} y1={0} y2={0} className="npcgen-axis" vectorEffect="non-scaling-stroke" />
      <line y1={box.y} y2={box.y + box.h} x1={0} x2={0} className="npcgen-axis" vectorEffect="non-scaling-stroke" />
      {view.resources.map((point) => shape(point, "resources", COLORS.resources))}
      {view.objects.map((point) => shape(point, "objects", COLORS.objects))}
      {view.areas.map((point) => shape(point, "areas", COLORS.areas[point.kind] ?? "#c08a2e"))}
    </svg>
    <div className="npcgen-legend"><span><i style={{ background: COLORS.areas[0] }} />Monsters</span><span><i style={{ background: COLORS.areas[1] }} />NPCs</span><span><i style={{ background: COLORS.areas[2] }} />Interaction</span><span><i style={{ background: COLORS.resources }} />Resources</span><span><i style={{ background: COLORS.objects }} />Objects</span><span className="muted">Wheel: zoom · drag: move · double-click: fit · north is up</span></div>
  </div>;
}

// ── The workspace ──

export const NpcGenEditor = forwardRef<NpcGenEditorHandle, Props>(function NpcGenEditor({ active, onStateChange, icon, elementsPath = null }, ref) {
  const [view, setView] = useState<NpcGenView | null>(null);
  const [section, setSection] = useState<NpcGenSection>("areas");
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
  useEffect(() => {
    if (!view || !elementsPath) return;
    const ids = [...new Set([...view.areas, ...view.resources].flatMap((entry) => entry.ids))].filter((id) => id && !(String(id) in labels));
    if (ids.length) dynTaskLabels(ids, []).then((found) => setLabels((current) => ({ ...current, ...found.elements }))).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view, elementsPath]);

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
  const pickSearch = useMemo(() => picking ? (text: string, page: number) => pickEssence(picking.kind, text, page, picking.current || null) : undefined, [picking]);

  const undo = useCallback(() => { if (view?.canUndo) void run(undoNpcGen); }, [run, view?.canUndo]);
  const redo = useCallback(() => { if (view?.canRedo) void run(redoNpcGen); }, [run, view?.canRedo]);
  const cloneSelected = useCallback(() => {
    if (selected === null) return;
    void run(async () => { const result = await cloneNpcGenItem(section, selected); setSelected(result.index); return result.view; });
  }, [run, section, selected]);
  const deleteSelected = useCallback(() => {
    if (selected === null) return;
    const one = SECTIONS.find((entry) => entry.key === section)!.one;
    if (!window.confirm(`Delete ${one} ${selected + 1}? Undo brings it back.`)) return;
    void run(async () => { const next = await deleteNpcGenItem(section, selected); const total = next[section].length; setSelected(total ? Math.min(selected, total - 1) : null); return next; });
  }, [run, section, selected]);
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

  useImperativeHandle(ref, () => ({ choose: () => void choose(), openPath: (path) => void load(path), save: saveCurrent, saveAs: () => void saveAs(), undo, redo, cloneSelected, deleteSelected, toggleMap }), [choose, cloneSelected, deleteSelected, load, redo, saveAs, saveCurrent, toggleMap, undo]);

  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      const mod = event.ctrlKey || event.metaKey;
      const typing = event.target instanceof HTMLElement && /^(INPUT|TEXTAREA|SELECT)$/.test(event.target.tagName);
      const key = event.key.toLowerCase();
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
  }, [active, choose, cloneSelected, deleteSelected, redo, saveAs, saveCurrent, undo]);

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

  const generators = view.areas.reduce((total, entry) => total + entry.ids.length, 0);
  const rowLabel = (entry: NpcGenSummary) => {
    if (section === "controllers") return <><span className="mono">{entry.ids[0]}</span><span className="truncate">{entry.label || <span className="muted">(no name)</span>}{entry.controller !== 0 && <span className="muted small"> · trigger {entry.controller}</span>}</span>{entry.kind !== 0 ? <span className="tag ok">on</span> : <span />}<span /></>;
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
    const common = { version: view.version, labels, elementsOpen: !!elementsPath, controllers: view.controllers, pick };
    switch (item.section) {
      case "areas": return <AreaForm key={key} {...common} value={item.item} onCommit={(value, label) => commit({ section: "areas", item: value }, label)} />;
      case "resources": return <ResourceForm key={key} {...common} value={item.item} onCommit={(value, label) => commit({ section: "resources", item: value }, label)} />;
      case "objects": return <ObjectForm key={key} {...common} value={item.item} onCommit={(value, label) => commit({ section: "objects", item: value }, label)} />;
      default: return <ControllerForm key={key} {...common} value={item.item} onCommit={(value, label) => commit({ section: "controllers", item: value }, label)} />;
    }
  })() : null;

  return <section className="dyn-tasks-pane" aria-busy={busy}>
    <header className="tasks-head">
      <div><h2>NPC generator {view.dirty && <span className="tag warn">unsaved</span>}</h2><div className="tasks-file-line">
        <span className="mono truncate" title={view.path}>{view.path}</span>
        <span className="path-data-badge"><b>Version:</b> {view.version}</span>
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
        {showMap && section !== "controllers" && <MapPlot view={view} section={section} selected={selected} onSelect={(kind, index) => { setSection(kind); setSelected(index); }} />}
        <div className="dyn-task-form-scroll">
          {selected === null ? <div className="empty-note">Select a {SECTIONS.find((entry) => entry.key === section)!.one}{section !== "controllers" ? " in the list or on the map" : ""}.</div>
            : <><div className="dyn-task-title"><h3>{SECTIONS.find((entry) => entry.key === section)!.one} {selected + 1}</h3></div>{form ?? <div className="empty-note">Loading…</div>}</>}
        </div>
      </div>
    </div>
    {picking && pickSearch && <ValuePicker search={pickSearch} icon={icon} onApply={async (value) => { picking.resolve(Number(value)); return null; }} onClose={() => { picking.resolve(null); setPicking(null); }} />}
    {saving && <div className="modal-backdrop" onMouseDown={() => !busy && setSaving(null)}>
      <div className="modal dyn-save-dialog" onMouseDown={(event) => event.stopPropagation()}>
        <h3>Save npcgen.data</h3>
        <p className="mono truncate" title={saving.target ?? view.path}>{saving.target ?? view.path}</p>
        <p className="muted small">The file keeps its version ({view.version}). The server reads it when the map starts, so restart that map's server.</p>
        {saving.changed && <div className="path-data-message error">Another program changed this file since it was opened. Replacing it discards those changes.</div>}
        <label className="small"><input type="checkbox" checked={backup} onChange={(event) => { setBackup(event.target.checked); try { localStorage.setItem(BACKUP_KEY, event.target.checked ? "1" : "0"); } catch { /* optional */ } }} /> Back up the existing file first (<span className="mono">.bak</span>, once per session)</label>
        <footer><span className="spacer" /><button className="btn" onClick={() => setSaving(null)} disabled={busy}>Cancel</button><button className="btn primary" onClick={() => void writeTo(saving.target, saving.changed)} disabled={busy}>{busy ? <Loader2 size={14} className="spin" /> : <Save size={14} />} {saving.changed ? "Replace anyway" : "Save"}</button></footer>
      </div>
    </div>}
  </section>;
});
