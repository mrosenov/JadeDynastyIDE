import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState, type ReactNode } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, ArrowDown, ArrowUp, CircleAlert, ClipboardPaste, Copy, FolderOpen, Info, LandPlot, Loader2, Plus, Redo2, Save, Search, Server, Trash2, TriangleAlert, Undo2, X } from "lucide-react";
import { clientMaps, mapRegionsProblems, midmapUrl, openMapRegions, readMapSev, saveMapRegions } from "../elements/api";
import { count } from "../elements/format";
import type { ClientMap, MapBuffRegion, MapPrecinct, MapRegionFile, MapRegionItem, MapRegionProblem, MapVec3, OpenedMapRegions } from "../elements/types";
import { NumberInput, TextInput, VertInput } from "./DynTasksEditor";

export interface MapRegionsEditorState {
  loaded: boolean;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  path: string | null;
  kind: "precinct" | "region" | null;
  entries: number;
}

export interface MapRegionsEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
  save: () => void;
  chooseServer: () => void;
  undo: () => void;
  redo: () => void;
  cloneSelected: () => void;
  deleteSelected: () => void;
  openProblems: () => void;
}

interface Props {
  active: boolean;
  onStateChange: (state: MapRegionsEditorState) => void;
  /** The client's image generation (null: no client folder, no map images). */
  mapGeneration: number | null;
}

interface History {
  states: MapRegionFile[];
  cursor: number;
  keys: string[];
}

/** precinct: index into precincts; item: into the region file's items; buff: into its buff regions. */
type Target = "precinct" | "item" | "buff";
interface Selection { target: Target; index: number }
type RegionTab = "region" | "portal" | "guildBuilding" | "buff";

const LAST_KEY = "jdide.maps.lastPath";
const BACKUP_KEY = "jdide.maps.backup";
const SERVER_KEY = "jdide.maps.serverFor";
const SERVER_ROOT_KEY = "jdide.maps.serverRoot";
const I32 = 2147483647;
const messageOf = (problem: unknown) => String(problem).replace(/^Error: /, "");
const zero = (): MapVec3 => ({ x: 0, y: 0, z: 0 });
const TABS: { key: RegionTab; label: string }[] = [{ key: "region", label: "Regions" }, { key: "portal", label: "Portals" }, { key: "guildBuilding", label: "Guild buildings" }, { key: "buff", label: "Buff regions" }];
const COLORS = { safe: "#2e9e5b", pk: "#d9534f", portal: "#e08a1e", buff: "#9b59b6", guild: "#888", precinct: ["#3b82c4", "#c08a2e", "#16a2a2", "#b0507a", "#6a8f2a", "#7a62c4"] };

function readJson<T>(key: string, fallback: T): T {
  try { const raw = localStorage.getItem(key); return raw ? JSON.parse(raw) as T : fallback; } catch { return fallback; }
}
function writeJson(key: string, value: unknown) {
  try { localStorage.setItem(key, JSON.stringify(value)); } catch { /* optional */ }
}
const serverFor = (path: string): string | null => readJson<Record<string, string>>(SERVER_KEY, {})[path.toLowerCase()] ?? null;
const rememberServer = (path: string, server: string) => {
  writeJson(SERVER_KEY, { ...readJson<Record<string, string>>(SERVER_KEY, {}), [path.toLowerCase()]: server });
  const parts = server.split(/[\\/]/);
  if (parts.length > 2) writeJson(SERVER_ROOT_KEY, parts.slice(0, -2).join("\\"));
};

/** A labelled row of the forms. */
function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return <>
    <span className="npcgen-label" title={hint}>{label}{hint && <span className="npcgen-hint">?</span>}</span>
    <span className="maps-value">{children}</span>
  </>;
}

function Card({ title, hint, children, actions }: { title: string; hint?: string; children: ReactNode; actions?: ReactNode }) {
  return <section className="dyn-section">
    <header><b>{title}</b>{hint && <span className="muted small">{hint}</span>}<span className="spacer" />{actions}</header>
    <div className="dyn-section-body">{children}</div>
  </section>;
}

function Int({ value, onCommit, min = -I32 - 1, title }: { value: number; onCommit: (value: number) => void; min?: number; title?: string }) {
  return <NumberInput value={value} min={min} max={I32} title={title} onCommit={onCommit} />;
}

/** An instance by ID, named from the client's instance.txt. */
function InstanceInput({ value, maps, onCommit }: { value: number; maps: ClientMap[]; onCommit: (value: number) => void }) {
  const named = maps.find((map) => map.id === value);
  return <span className="maps-instance">
    <Int value={value} onCommit={onCommit} />
    {maps.length > 0 && <select className="task-form-select" value={named ? value : ""} onChange={(event) => { if (event.target.value) onCommit(Number(event.target.value)); }}>
      {!named && <option value="">{value} · not in instance.txt</option>}
      {maps.map((map) => <option key={map.id} value={map.id}>{map.id} · {map.name}</option>)}
    </select>}
  </span>;
}

/** The outline points: a table of x, y, z with insert, remove, move and paste. */
function PointsTable({ points, onCommit }: { points: MapVec3[]; onCommit: (points: MapVec3[], key: string) => void }) {
  const [paste, setPaste] = useState<string | null>(null);
  const parsed = useMemo(() => {
    if (paste === null) return null;
    const rows = paste.split(/\r?\n/).map((line) => line.trim()).filter(Boolean).map((line) => line.split(/[\s,;]+/).map(Number));
    return rows.every((row) => row.length === 3 && row.every(Number.isFinite)) ? rows.map(([x, y, z]) => ({ x: Math.fround(x), y: Math.fround(y), z: Math.fround(z) })) : null;
  }, [paste]);
  const set = (next: MapVec3[]) => onCommit(next, `points:${Date.now()}`);
  return <div className="maps-points">
    <table className="dyn-table">
      <thead><tr><th>#</th><th>X</th><th>Y (height)</th><th>Z</th><th /></tr></thead>
      <tbody>{points.map((point, index) => <tr key={index}>
        <td className="mono muted">{index + 1}</td>
        {(["x", "y", "z"] as const).map((axis) => <td key={axis}><NumberInput float min={-1e9} max={1e9} value={point[axis]} onCommit={(value) => onCommit(points.map((entry, at) => at === index ? { ...entry, [axis]: value } : entry), `point:${index}:${axis}`)} /></td>)}
        <td className="nowrap">
          <button className="icon-btn small" title="Insert a point after this one (halfway to the next)" onClick={() => { const next = points[(index + 1) % points.length]; const mid = { x: Math.fround((point.x + next.x) / 2), y: Math.fround((point.y + next.y) / 2), z: Math.fround((point.z + next.z) / 2) }; set([...points.slice(0, index + 1), mid, ...points.slice(index + 1)]); }}><Plus size={12} /></button>
          <button className="icon-btn small" title="Move up" disabled={index === 0} onClick={() => { const next = [...points]; [next[index - 1], next[index]] = [next[index], next[index - 1]]; set(next); }}><ArrowUp size={12} /></button>
          <button className="icon-btn small" title="Move down" disabled={index + 1 >= points.length} onClick={() => { const next = [...points]; [next[index + 1], next[index]] = [next[index], next[index + 1]]; set(next); }}><ArrowDown size={12} /></button>
          <button className="icon-btn small danger" title="Remove" onClick={() => set(points.filter((_, at) => at !== index))}><Trash2 size={12} /></button>
        </td>
      </tr>)}</tbody>
    </table>
    <div className="maps-points-actions">
      <button className="btn small" onClick={() => set([...points, points.length ? { ...points[points.length - 1] } : zero()])}><Plus size={12} /> Add a point</button>
      <button className="btn small" onClick={() => setPaste(points.map((point) => `${point.x} ${point.y} ${point.z}`).join("\n"))}><ClipboardPaste size={12} /> Edit as text…</button>
    </div>
    {paste !== null && <div className="maps-paste">
      <textarea className="dyn-textarea mono" rows={Math.min(12, Math.max(4, paste.split("\n").length + 1))} value={paste} onChange={(event) => setPaste(event.target.value)} />
      <div className="maps-points-actions">
        <span className={"small " + (parsed ? "muted" : "ai-warn")}>{parsed ? `${parsed.length} points (x y z per line)` : "Each line needs three numbers: x y z"}</span>
        <span className="spacer" />
        <button className="btn small" onClick={() => setPaste(null)}>Cancel</button>
        <button className="btn small primary" disabled={!parsed} onClick={() => { set(parsed!); setPaste(null); }}>Use these points</button>
      </div>
    </div>}
  </div>;
}

// ---------------------------------------------------------------- map preview

interface Shape {
  key: string;
  target: Target | null;
  index: number;
  points?: MapVec3[];
  box?: { pos: MapVec3; extents: MapVec3 };
  dot?: MapVec3;
  color: string;
  dashed?: boolean;
  faded?: boolean;
  title: string;
}

/** The x/z bounds of a shape. */
function shapeBounds(shape: Shape) {
  const points = shape.points ?? (shape.box ? [{ ...shape.box.pos, x: shape.box.pos.x - shape.box.extents.x, z: shape.box.pos.z - shape.box.extents.z }, { ...shape.box.pos, x: shape.box.pos.x + shape.box.extents.x, z: shape.box.pos.z + shape.box.extents.z }] : shape.dot ? [shape.dot] : []);
  if (!points.length) return null;
  const xs = points.map((point) => point.x), zs = points.map((point) => point.z);
  return { minX: Math.min(...xs), maxX: Math.max(...xs), minZ: Math.min(...zs), maxZ: Math.max(...zs) };
}

function MapPreview({ shapes, selected, onSelect, background, onImageError, extras, controls, focus }: { shapes: Shape[]; selected: Selection | null; onSelect: (selection: Selection) => void; background: { url: string; half: number } | null; onImageError: () => void; extras: { key: string; at: MapVec3; label: string; kind: "city" | "kick" | "mark" | "target" }[]; controls: ReactNode; focus: number }) {
  const bounds = useMemo(() => {
    if (background) return { x: -background.half, y: -background.half, w: background.half * 2, h: background.half * 2 };
    let minX = Infinity, maxX = -Infinity, minZ = Infinity, maxZ = -Infinity;
    const take = (point: MapVec3) => { minX = Math.min(minX, point.x); maxX = Math.max(maxX, point.x); minZ = Math.min(minZ, point.z); maxZ = Math.max(maxZ, point.z); };
    for (const shape of shapes) { shape.points?.forEach(take); if (shape.box) take(shape.box.pos); if (shape.dot) take(shape.dot); }
    if (!Number.isFinite(minX)) return { x: -512, y: -512, w: 1024, h: 1024 };
    const pad = Math.max(20, (maxX - minX + maxZ - minZ) * 0.04);
    return { x: minX - pad, y: -maxZ - pad, w: maxX - minX + pad * 2, h: maxZ - minZ + pad * 2 };
  }, [background, shapes]);
  const [box, setBox] = useState(bounds);
  // Fit when another file or map is shown, not after every edit.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => setBox(bounds), [background?.url, background?.half, shapes.length === 0]);
  const svg = useRef<SVGSVGElement>(null);
  const drag = useRef<{ x: number; y: number; box: typeof box; moved: boolean } | null>(null);
  const chosen = shapes.find((shape) => !!selected && shape.target === selected.target && shape.index === selected.index && !shape.faded) ?? null;
  const chosenBounds = chosen ? shapeBounds(chosen) : null;
  // Picked from the list: show it (at least 160 m across) unless it is already in view at a useful size.
  useEffect(() => {
    if (!focus || !chosenBounds) return;
    const width = Math.max(160, (chosenBounds.maxX - chosenBounds.minX) * 1.4), height = Math.max(160, (chosenBounds.maxZ - chosenBounds.minZ) * 1.4);
    const cx = (chosenBounds.minX + chosenBounds.maxX) / 2, cz = (chosenBounds.minZ + chosenBounds.maxZ) / 2;
    const size = Math.max(width, height);
    setBox({ x: cx - size / 2, y: -cz - size / 2, w: size, h: size });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus]);
  const toWorld = (clientX: number, clientY: number, current = box) => {
    const rect = svg.current!.getBoundingClientRect();
    const scale = Math.max(current.w / rect.width, current.h / rect.height);
    const offsetX = (rect.width * scale - current.w) / 2, offsetY = (rect.height * scale - current.h) / 2;
    return { x: current.x - offsetX + (clientX - rect.left) * scale, y: current.y - offsetY + (clientY - rect.top) * scale, scale };
  };
  const unit = box.w / 600;
  const isSelected = (shape: Shape) => !!selected && shape.target === selected.target && shape.index === selected.index;
  const ordered = [...shapes].sort((a, b) => Number(isSelected(a)) - Number(isSelected(b)) || Number(!a.faded) - Number(!b.faded));
  return <div className="npcgen-map maps-map">
    <div className="npcgen-mapbar">{controls}</div>
    <svg ref={svg} viewBox={`${box.x} ${box.y} ${box.w} ${box.h}`} preserveAspectRatio="xMidYMid meet"
      onWheel={(event) => { const at = toWorld(event.clientX, event.clientY); const factor = event.deltaY > 0 ? 1.2 : 1 / 1.2; setBox((current) => ({ x: at.x - (at.x - current.x) * factor, y: at.y - (at.y - current.y) * factor, w: current.w * factor, h: current.h * factor })); }}
      onMouseDown={(event) => { drag.current = { x: event.clientX, y: event.clientY, box, moved: false }; }}
      onMouseMove={(event) => { const start = drag.current; if (!start || !svg.current) return; if (Math.abs(event.clientX - start.x) + Math.abs(event.clientY - start.y) > 3) start.moved = true; const scale = toWorld(0, 0, start.box).scale; setBox({ ...start.box, x: start.box.x - (event.clientX - start.x) * scale, y: start.box.y - (event.clientY - start.y) * scale }); }}
      onMouseUp={() => { setTimeout(() => { drag.current = null; }); }} onMouseLeave={() => { drag.current = null; }} onDoubleClick={() => setBox(bounds)}>
      {background && <image href={background.url} x={-background.half} y={-background.half} width={background.half * 2} height={background.half * 2} preserveAspectRatio="none" pointerEvents="none" onError={onImageError} />}
      {!background && <>
        <line x1={box.x} x2={box.x + box.w} y1={0} y2={0} className="npcgen-axis" vectorEffect="non-scaling-stroke" />
        <line y1={box.y} y2={box.y + box.h} x1={0} x2={0} className="npcgen-axis" vectorEffect="non-scaling-stroke" />
      </>}
      {ordered.map((shape) => {
        const active = isSelected(shape);
        const common = {
          className: shape.target && !shape.faded ? "npcgen-shape" : undefined,
          pointerEvents: shape.faded || !shape.target ? ("none" as const) : undefined,
          onClick: (event: React.MouseEvent) => { event.stopPropagation(); if (drag.current?.moved || !shape.target) return; onSelect({ target: shape.target, index: shape.index }); },
          stroke: active ? "var(--accent)" : shape.color,
          strokeWidth: active ? 3 : 1.5,
          strokeDasharray: shape.dashed ? "6 4" : undefined,
          vectorEffect: "non-scaling-stroke" as const,
          opacity: shape.faded ? 0.7 : 1,
        };
        if (shape.points) return <polygon key={shape.key} {...common} points={shape.points.map((point) => `${point.x},${-point.z}`).join(" ")} fill={shape.color} fillOpacity={active ? 0.35 : shape.faded ? 0.04 : 0.14}><title>{shape.title}</title></polygon>;
        if (shape.box) { const { pos, extents } = shape.box; return <rect key={shape.key} {...common} x={pos.x - extents.x} y={-pos.z - extents.z} width={Math.max(extents.x * 2, unit * 2)} height={Math.max(extents.z * 2, unit * 2)} fill={shape.color} fillOpacity={active ? 0.5 : 0.3}><title>{shape.title}</title></rect>; }
        if (shape.dot) return <rect key={shape.key} {...common} x={shape.dot.x - unit * 3} y={-shape.dot.z - unit * 3} width={unit * 6} height={unit * 6} fill={shape.color}><title>{shape.title}</title></rect>;
        return null;
      })}
      {chosenBounds && <circle cx={(chosenBounds.minX + chosenBounds.maxX) / 2} cy={-(chosenBounds.minZ + chosenBounds.maxZ) / 2} r={Math.max(unit * 14, Math.hypot(chosenBounds.maxX - chosenBounds.minX, chosenBounds.maxZ - chosenBounds.minZ) / 2 + unit * 6)} className="maps-ring" vectorEffect="non-scaling-stroke" pointerEvents="none" />}
      {extras.map((extra) => {
        const size = unit * (extra.kind === "mark" ? 2.5 : 5);
        const x = extra.at.x, y = -extra.at.z;
        return <g key={extra.key} pointerEvents="none" className={"maps-extra " + extra.kind}>
          {extra.kind === "kick" ? <path d={`M${x - size} ${y - size}L${x + size} ${y + size}M${x - size} ${y + size}L${x + size} ${y - size}`} strokeWidth={2} vectorEffect="non-scaling-stroke" />
            : extra.kind === "mark" ? <circle cx={x} cy={y} r={size} />
            : <path d={`M${x} ${y - size * 1.3}L${x + size} ${y}L${x} ${y + size * 1.3}L${x - size} ${y}Z`} />}
          <text x={x + size * 1.6} y={y + size * 0.6} fontSize={unit * (extra.kind === "mark" ? 9 : 11)}>{extra.label}</text>
        </g>;
      })}
    </svg>
  </div>;
}

// ---------------------------------------------------------------- editor

/** A map's precinct.clt or region.clt, saved together with its server copy (.sev). */
export const MapRegionsEditor = forwardRef<MapRegionsEditorHandle, Props>(function MapRegionsEditor({ active, onStateChange, mapGeneration }, ref) {
  const [meta, setMeta] = useState<Omit<OpenedMapRegions, "file"> | null>(null);
  const [history, setHistory] = useState<History>({ states: [], cursor: -1, keys: [] });
  const [selected, setSelected] = useState<Selection | null>(null);
  const [tab, setTab] = useState<RegionTab>("region");
  const [query, setQuery] = useState("");
  const [instanceFilter, setInstanceFilter] = useState<number | null>(null);
  const [showSibling, setShowSibling] = useState(true);
  const [showImage, setShowImage] = useState(true);
  const [server, setServer] = useState<{ path: string; file: MapRegionFile | null; error: string | null } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [backup, setBackup] = useState(() => { try { return localStorage.getItem(BACKUP_KEY) !== "0"; } catch { return true; } });
  const [problems, setProblems] = useState<MapRegionProblem[] | null>(null);
  const [problemsOpen, setProblemsOpen] = useState(false);
  const [maps, setMaps] = useState<ClientMap[]>([]);
  const [failedImage, setFailedImage] = useState<string | null>(null);
  /** Changes when an entry is picked from the list or the problems, so the map shows it. */
  const [focus, setFocus] = useState(0);
  const pick = (selection: Selection) => { setSelected(selection); setFocus((current) => current + 1); };
  const saved = useRef<MapRegionFile | null>(null);
  const autoOpened = useRef(false);
  const backedUp = useRef(new Set<string>());

  const file = history.cursor >= 0 ? history.states[history.cursor] : null;
  const dirty = !!file && file !== saved.current;

  useEffect(() => {
    if (mapGeneration === null) { setMaps([]); return; }
    let cancelled = false;
    clientMaps().then((found) => { if (!cancelled) setMaps(found); }).catch(() => { if (!cancelled) setMaps([]); });
    return () => { cancelled = true; };
  }, [mapGeneration]);

  const loadServer = useCallback(async (serverPath: string, kind: "precinct" | "region") => {
    try {
      const parsed = await readMapSev(serverPath, kind);
      setServer({ path: serverPath, file: parsed, error: null });
      return parsed;
    } catch (problem) {
      setServer({ path: serverPath, file: null, error: messageOf(problem) });
      return null;
    }
  }, []);

  const load = useCallback(async (path: string) => {
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const opened = await openMapRegions(path);
      let initial = opened.file;
      const remembered = serverFor(opened.path);
      setServer(null);
      if (remembered) {
        const parsed = await loadServer(remembered, initial.kind);
        // The client file cannot hold buff regions: they come from the server file.
        if (parsed?.kind === "region" && initial.kind === "region") initial = { ...initial, buffs: parsed.buffs };
      }
      const { file: _file, ...rest } = opened;
      saved.current = initial;
      setMeta(rest);
      setHistory({ states: [initial], cursor: 0, keys: [] });
      setSelected(initial.kind === "precinct" ? (initial.precincts.length ? { target: "precinct", index: 0 } : null) : null);
      setTab("region");
      setQuery("");
      setInstanceFilter(null);
      setFailedImage(null);
      try { localStorage.setItem(LAST_KEY, opened.path); } catch { /* optional */ }
    } catch (problem) {
      setError(messageOf(problem));
    } finally {
      setBusy(false);
    }
  }, [loadServer]);
  useEffect(() => {
    if (autoOpened.current) return;
    autoOpened.current = true;
    let last: string | null = null;
    try { last = localStorage.getItem(LAST_KEY); } catch { /* optional */ }
    if (last) void load(last);
  }, [load]);
  const guard = useCallback((action: () => void) => {
    if (!dirty || window.confirm("Discard the unsaved changes to this map file?")) action();
  }, [dirty]);
  const choose = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, defaultPath: meta?.path, title: "Open a map's precinct.clt or region.clt (element\\maps\\<map>)", filters: [{ name: "precinct.clt, region.clt", extensions: ["clt"] }] });
    if (typeof picked === "string") guard(() => void load(picked));
  }, [guard, load, meta?.path]);

  const commit = useCallback((next: MapRegionFile, key: string) => {
    setHistory((current) => {
      if (current.cursor < 0) return current;
      const states = current.states.slice(0, current.cursor + 1);
      const keys = current.keys.slice(0, current.cursor);
      if (current.cursor > 0 && keys[current.cursor - 1] === key) { states[current.cursor] = next; return { states, cursor: current.cursor, keys }; }
      states.push(next);
      keys.push(key);
      if (states.length > 301) { states.shift(); keys.shift(); return { states, cursor: states.length - 1, keys }; }
      return { states, cursor: current.cursor + 1, keys };
    });
    setNote(null);
  }, []);
  const undo = useCallback(() => setHistory((current) => current.cursor > 0 ? { ...current, cursor: current.cursor - 1 } : current), []);
  const redo = useCallback(() => setHistory((current) => current.cursor + 1 < current.states.length ? { ...current, cursor: current.cursor + 1 } : current), []);

  const chooseServer = useCallback(async () => {
    if (!file || !meta) return;
    const root = readJson<string | null>(SERVER_ROOT_KEY, null);
    const proposal = server?.path ?? (root ? `${root}\\${meta.folder}\\${file.kind}.sev` : `${file.kind}.sev`);
    const picked = await save({ defaultPath: proposal, title: `The server copy of this file: ${file.kind}.sev (gamed\\config\\<map>), written on every save`, filters: [{ name: `${file.kind}.sev`, extensions: ["sev"] }] });
    if (typeof picked !== "string") return null;
    rememberServer(meta.path, picked);
    const parsed = await loadServer(picked, file.kind);
    if (parsed?.kind === "region" && file.kind === "region" && parsed.buffs.length && !file.buffs.length) {
      commit({ ...file, buffs: parsed.buffs }, `server-buffs:${Date.now()}`);
      setNote(`Took ${parsed.buffs.length} buff region${parsed.buffs.length === 1 ? "" : "s"} from the server file (they exist only there). Undo takes them out again.`);
    }
    return picked;
  }, [commit, file, loadServer, meta, server?.path]);

  useEffect(() => {
    if (!file) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      mapRegionsProblems(file, server?.file ?? null, maps.map((map) => map.id)).then((found) => { if (!cancelled) setProblems(found); }).catch((problem) => { if (!cancelled) setError(messageOf(problem)); });
    }, 350);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [file, server?.file, maps]);

  const saveTo = useCallback(async (replaceChanged = false): Promise<void> => {
    if (!file || !meta || busy) return;
    let serverPath = server?.path ?? null;
    if (!serverPath) {
      if (!window.confirm(`Saving also writes the server copy (${file.kind}.sev) with the same new timestamp. Choose where it goes (the server's gamed\\config\\${meta.folder} folder).`)) return;
      serverPath = (await chooseServer()) ?? null;
      if (!serverPath) return;
    }
    const errors = (problems ?? []).filter((problem) => problem.severity === "error" && !problem.message.includes("saving writes") && !problem.message.includes("until both are saved"));
    if (!replaceChanged && errors.length && !window.confirm(`These problems remain:\n\n${errors.slice(0, 6).map((problem) => `• ${problem.message}`).join("\n")}\n\nSave anyway?`)) { setProblemsOpen(true); return; }
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const key = meta.path.toLowerCase();
      const report = await saveMapRegions(meta.path, meta.token, file, serverPath, backup && !backedUp.current.has(key), replaceChanged);
      backedUp.current.add(key);
      const stamped = { ...file, timestamp: report.timestamp } as MapRegionFile;
      saved.current = stamped;
      setHistory((current) => ({ ...current, states: current.states.map((state, index) => index === current.cursor ? stamped : state) }));
      setMeta((current) => current && { ...current, token: report.token });
      await loadServer(report.serverPath, file.kind);
      setNote(`Saved ${report.path} and ${report.serverPath} with timestamp ${report.timestamp}.${report.backups.length ? ` Backups: ${report.backups.join(", ")}.` : ""} Copy the .clt to the clients and restart the map's game server.`);
    } catch (problem) {
      const message = messageOf(problem);
      if (message.startsWith("CHANGED_ON_DISK:") && window.confirm(`${message.slice("CHANGED_ON_DISK:".length).trim()}\n\nReplace it with the open values?`)) {
        setBusy(false);
        await saveTo(true);
        return;
      }
      setError(message);
    } finally {
      setBusy(false);
    }
  }, [backup, busy, chooseServer, file, loadServer, meta, problems, server?.path]);
  const saveCurrent = useCallback(() => { if (meta && (dirty || server?.file?.timestamp !== file?.timestamp)) void saveTo(); }, [dirty, file?.timestamp, meta, saveTo, server?.file?.timestamp]);

  // ---------------------------------------------------------------- entries

  const setPrecinct = (index: number, change: Partial<MapPrecinct>, key: string) => {
    if (file?.kind !== "precinct") return;
    commit({ ...file, precincts: file.precincts.map((entry, at) => at === index ? { ...entry, ...change } : entry) }, `p${index}:${key}`);
  };
  const setItem = (index: number, item: MapRegionItem, key: string) => {
    if (file?.kind !== "region") return;
    commit({ ...file, items: file.items.map((entry, at) => at === index ? item : entry) }, `i${index}:${key}`);
  };
  const setBuff = (index: number, change: Partial<MapBuffRegion>, key: string) => {
    if (file?.kind !== "region") return;
    commit({ ...file, buffs: file.buffs.map((entry, at) => at === index ? { ...entry, ...change } : entry) }, `b${index}:${key}`);
  };
  /** Copies or removes the selected entry (portals keep their order: it is their number on the server). */
  const cloneSelected = useCallback(() => {
    if (!file || !selected) return;
    if (file.kind === "precinct" && selected.target === "precinct") {
      const source = file.precincts[selected.index];
      const id = source.id ? Math.max(...file.precincts.map((entry) => entry.id)) + 1 : 0;
      const copy = structuredClone(source);
      copy.id = id;
      copy.name = `${source.name} (copy)`;
      commit({ ...file, precincts: [...file.precincts.slice(0, selected.index + 1), copy, ...file.precincts.slice(selected.index + 1)] }, `clone:${Date.now()}`);
      setSelected({ target: "precinct", index: selected.index + 1 });
    } else if (file.kind === "region" && selected.target === "item") {
      const copy = structuredClone(file.items[selected.index]);
      if (copy.kind === "region") copy.name = `${copy.name} (copy)`;
      commit({ ...file, items: [...file.items.slice(0, selected.index + 1), copy, ...file.items.slice(selected.index + 1)] }, `clone:${Date.now()}`);
      setSelected({ target: "item", index: selected.index + 1 });
      if (copy.kind === "portal") setNote("Portals after the copy moved one number on; saving writes the client and server files together, so they stay in step.");
    } else if (file.kind === "region" && selected.target === "buff") {
      const copy = structuredClone(file.buffs[selected.index]);
      commit({ ...file, buffs: [...file.buffs.slice(0, selected.index + 1), copy, ...file.buffs.slice(selected.index + 1)] }, `clone:${Date.now()}`);
      setSelected({ target: "buff", index: selected.index + 1 });
    }
  }, [commit, file, selected]);
  const deleteSelected = useCallback(() => {
    if (!file || !selected) return;
    if (!window.confirm("Delete the selected entry? Undo brings it back.")) return;
    if (file.kind === "precinct" && selected.target === "precinct") commit({ ...file, precincts: file.precincts.filter((_, at) => at !== selected.index) }, `delete:${Date.now()}`);
    else if (file.kind === "region" && selected.target === "item") commit({ ...file, items: file.items.filter((_, at) => at !== selected.index) }, `delete:${Date.now()}`);
    else if (file.kind === "region" && selected.target === "buff") commit({ ...file, buffs: file.buffs.filter((_, at) => at !== selected.index) }, `delete:${Date.now()}`);
    setSelected(null);
  }, [commit, file, selected]);
  const move = (delta: number) => {
    if (!file || !selected) return;
    const swap = <T,>(list: T[], from: number, to: number) => { const next = [...list]; [next[from], next[to]] = [next[to], next[from]]; return next; };
    if (file.kind === "precinct" && selected.target === "precinct") {
      const to = selected.index + delta;
      if (to < 0 || to >= file.precincts.length) return;
      commit({ ...file, precincts: swap(file.precincts, selected.index, to) }, `move:${Date.now()}`);
      setSelected({ target: "precinct", index: to });
    } else if (file.kind === "region" && selected.target === "item") {
      // Move past the next entry of the same kind.
      const kind = file.items[selected.index].kind;
      let to = selected.index + delta;
      while (to >= 0 && to < file.items.length && file.items[to].kind !== kind) to += delta;
      if (to < 0 || to >= file.items.length) return;
      const next = [...file.items];
      const [entry] = next.splice(selected.index, 1);
      next.splice(to, 0, entry);
      commit({ ...file, items: next }, `move:${Date.now()}`);
      setSelected({ target: "item", index: to });
    }
  };
  /** The first entry of a kind when the file has none (copies need one to start from). */
  const addFirst = () => {
    if (!file) return;
    const instance = instanceFilter ?? 1;
    const square = (size: number): MapVec3[] => [{ x: -size, y: 0, z: -size }, { x: size, y: 0, z: -size }, { x: size, y: 0, z: size }, { x: -size, y: 0, z: size }];
    if (file.kind === "precinct") {
      commit({ ...file, precincts: [...file.precincts, { name: "New area", id: Math.max(0, ...file.precincts.map((entry) => entry.id)) + 1, priority: 1, cityInstance: instance, musicInterval: 0, musicLoop: 0, instance, domain: 0, city: zero(), rebirths: 0, levelMax: 0, levelMin: 0, kickCity: 1, kickPos: zero(), points: square(50), marks: [], sound: "", music: [], soundNight: "", minimap: "" }] }, `add:${Date.now()}`);
      setSelected({ target: "precinct", index: file.precincts.length });
    } else if (tab === "buff") {
      commit({ ...file, buffs: [...file.buffs, { after: 1e9, template: 0, monster: 0, points: square(50) }] }, `add:${Date.now()}`);
      setSelected({ target: "buff", index: file.buffs.length });
    } else {
      const item: MapRegionItem = tab === "region" ? { kind: "region", name: "Safe zone", regionType: 1, points: square(50) }
        : tab === "portal" ? { kind: "portal", target: instance, instance, level: 0, line: 0, controller: 0, pos: zero(), extents: { x: 5, y: 5, z: 5 }, targetPos: zero() }
        : { kind: "guildBuilding", template: 0, width: 1, height: 1, pos: zero(), dir: { x: 0, y: 0, z: 1 }, up: { x: 0, y: 1, z: 0 } };
      commit({ ...file, items: [...file.items, item] }, `add:${Date.now()}`);
      setSelected({ target: "item", index: file.items.length });
    }
  };

  useImperativeHandle(ref, () => ({ choose: () => void choose(), openPath: (path) => guard(() => void load(path)), save: saveCurrent, chooseServer: () => void chooseServer(), undo, redo, cloneSelected, deleteSelected, openProblems: () => setProblemsOpen(true) }), [choose, chooseServer, cloneSelected, deleteSelected, guard, load, redo, saveCurrent, undo]);
  useEffect(() => onStateChange({ loaded: !!file, dirty, canUndo: history.cursor > 0, canRedo: history.cursor + 1 < history.states.length, path: meta?.path ?? null, kind: file?.kind ?? null, entries: file ? (file.kind === "precinct" ? file.precincts.length : file.items.length + file.buffs.length) : 0 }), [dirty, file, history.cursor, history.states.length, meta?.path, onStateChange]);

  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      const mod = event.ctrlKey || event.metaKey;
      const typing = event.target instanceof HTMLElement && /^(INPUT|TEXTAREA|SELECT)$/.test(event.target.tagName);
      const key = event.key.toLowerCase();
      if (event.key === "Escape" && problemsOpen) { event.preventDefault(); setProblemsOpen(false); return; }
      if (mod && event.shiftKey && key === "m") { event.preventDefault(); event.stopImmediatePropagation(); setProblemsOpen(true); return; }
      if (mod && ["o", "s", "z", "y", "d"].includes(key)) {
        if (typing && (key === "z" || key === "y")) return;
        event.preventDefault();
        event.stopImmediatePropagation();
        if (key === "o") void choose();
        else if (key === "s") saveCurrent();
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
  }, [active, choose, cloneSelected, deleteSelected, problemsOpen, redo, saveCurrent, undo]);

  const mapName = (id: number) => maps.find((map) => map.id === id)?.name;
  const folderMaps = useMemo(() => meta ? maps.filter((map) => map.path.toLowerCase() === meta.folder.toLowerCase()) : [], [maps, meta]);
  const instances = useMemo(() => {
    if (!file) return [];
    const ids = new Set<number>();
    if (file.kind === "precinct") file.precincts.forEach((entry) => ids.add(entry.instance));
    else file.items.forEach((item) => { if (item.kind === "portal") ids.add(item.instance); });
    folderMaps.forEach((map) => ids.add(map.id));
    return [...ids].sort((a, b) => a - b);
  }, [file, folderMaps]);
  const flagged = useMemo(() => {
    const out = new Map<string, string>();
    for (const problem of problems ?? []) if (problem.index !== null && problem.severity !== "note") {
      const key = `${problem.target}:${problem.index}`;
      if (problem.severity === "error" || !out.has(key)) out.set(key, problem.severity);
    }
    return out;
  }, [problems]);

  const rows = useMemo(() => {
    if (!file) return [];
    const text = query.trim().toLowerCase();
    const matches = (...parts: (string | number)[]) => !text || parts.some((part) => String(part).toLowerCase().includes(text));
    if (file.kind === "precinct") return file.precincts.map((entry, index) => ({ target: "precinct" as Target, index, label: entry.name || "(no name)", detail: `${entry.id ? `ID ${entry.id}` : "client only"} · instance ${entry.instance} · priority ${entry.priority}`, instance: entry.instance, tag: entry.id ? null : "client" }))
      .filter((row) => (instanceFilter === null || row.instance === instanceFilter) && matches(row.label, row.detail));
    if (tab === "buff") return file.buffs.map((buff, index) => ({ target: "buff" as Target, index, label: `Buff region ${index + 1}`, detail: `template ${buff.template} · monster ${buff.monster} · ${buff.points.length} points`, instance: null as number | null, tag: "server" as string | null })).filter((row) => matches(row.label, row.detail));
    return file.items.map((item, index) => ({ item, index })).filter(({ item }) => item.kind === tab).map(({ item, index }, number) => {
      if (item.kind === "region") return { target: "item" as Target, index, label: item.name || "(no name)", detail: `${item.regionType === 1 ? "safe zone" : item.regionType === 0 ? "PK region" : `type ${item.regionType}`} · ${item.points.length} points`, instance: null as number | null, tag: null as string | null };
      if (item.kind === "portal") return { target: "item" as Target, index, label: `Portal ${number + 1} → ${mapName(item.target) ?? `instance ${item.target}`}`, detail: `from instance ${item.instance}${item.level > 1 ? ` · level ${item.level}+` : ""}`, instance: item.instance, tag: null };
      return { target: "item" as Target, index, label: `Guild building ${number + 1}`, detail: `template ${item.template} · ${item.width}×${item.height}`, instance: null, tag: "client" };
    }).filter((row) => (instanceFilter === null || row.instance === null || row.instance === instanceFilter) && matches(row.label, row.detail));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [file, query, tab, instanceFilter, maps]);

  // The map: this file's shapes, the folder's other file faded.
  const { shapes, extras } = useMemo(() => {
    const shapes: Shape[] = [];
    const extras: { key: string; at: MapVec3; label: string; kind: "city" | "kick" | "mark" | "target" }[] = [];
    const showInstance = (id: number) => instanceFilter === null || id === instanceFilter;
    const draw = (source: MapRegionFile, faded: boolean) => {
      if (source.kind === "precinct") source.precincts.forEach((entry, index) => {
        if (!showInstance(entry.instance)) return;
        shapes.push({ key: `${faded ? "s" : ""}p${index}`, target: faded ? null : "precinct", index, points: entry.points, color: COLORS.precinct[index % COLORS.precinct.length], dashed: entry.id === 0, faded, title: `${entry.name}${entry.id ? ` (ID ${entry.id}, priority ${entry.priority})` : " (client only)"}` });
      });
      else {
        source.items.forEach((item, index) => {
          const key = `${faded ? "s" : ""}i${index}`;
          if (item.kind === "region") shapes.push({ key, target: faded ? null : "item", index, points: item.points, color: item.regionType === 0 ? COLORS.pk : COLORS.safe, faded, title: `${item.name} (${item.regionType === 0 ? "PK region" : "safe zone"})` });
          else if (item.kind === "portal" && showInstance(item.instance)) shapes.push({ key, target: faded ? null : "item", index, box: { pos: item.pos, extents: item.extents }, color: COLORS.portal, faded, title: `Portal to ${mapName(item.target) ?? `instance ${item.target}`}` });
          else if (item.kind === "guildBuilding") shapes.push({ key, target: faded ? null : "item", index, dot: item.pos, color: COLORS.guild, faded, title: `Guild building (template ${item.template})` });
        });
        source.buffs.forEach((buff, index) => shapes.push({ key: `${faded ? "s" : ""}b${index}`, target: faded ? null : "buff", index, points: buff.points, color: COLORS.buff, dashed: true, faded, title: `Buff region (template ${buff.template})` }));
      }
    };
    if (meta?.sibling && showSibling) draw(meta.sibling.file, true);
    if (file) draw(file, false);
    if (file && selected) {
      if (file.kind === "precinct" && selected.target === "precinct") {
        const entry = file.precincts[selected.index];
        if (entry) {
          if (instances.includes(entry.cityInstance) || entry.cityInstance === entry.instance) extras.push({ key: "city", at: entry.city, label: "Return city", kind: "city" });
          if (file.version >= 7 && (entry.levelMax > 0 || entry.levelMin > 0)) extras.push({ key: "kick", at: entry.kickPos, label: "Kick-out", kind: "kick" });
          entry.marks.forEach((mark, index) => extras.push({ key: `m${index}`, at: mark.pos, label: mark.name, kind: "mark" }));
        }
      } else if (file.kind === "region" && selected.target === "item") {
        const item = file.items[selected.index];
        if (item?.kind === "portal" && (item.target === item.instance || folderMaps.some((map) => map.id === item.target))) extras.push({ key: "target", at: item.targetPos, label: "Arrival", kind: "target" });
      }
    }
    return { shapes, extras };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [file, meta?.sibling, showSibling, selected, instanceFilter, maps, instances, folderMaps]);

  if (!file || !meta) return <section className="path-data-pane empty">
    <div className="drop-card path-data-empty">
      <LandPlot size={30} />
      <h2>Open a precinct.clt or region.clt</h2>
      <p className="muted">A map's areas (precinct.clt: names, music, return cities, level limits) or regions (region.clt: safe zones, PK areas, portals) from the client's <span className="mono">element\maps\&lt;map&gt;</span>. Saving also writes the server copy (<span className="mono">.sev</span> in <span className="mono">gamed\config\&lt;map&gt;</span>) with the same timestamp.</p>
      <button className="btn primary" onClick={() => void choose()} disabled={busy}>{busy ? <Loader2 size={14} className="spin" /> : <FolderOpen size={14} />} Choose a .clt file…</button>
      {error && <div className="se-problems" role="alert">{error}</div>}
    </div>
  </section>;

  const errors = (problems ?? []).filter((problem) => problem.severity === "error").length;
  const half = folderMaps.length ? Math.max(...folderMaps.map((map) => map.rows)) * 512 : 0;
  const imageMap = folderMaps.find((map) => map.hasImage);
  const imageUrl = imageMap && half > 0 && mapGeneration !== null ? midmapUrl(mapGeneration, imageMap.path) : null;
  const background = imageUrl && showImage && failedImage !== imageUrl ? { url: imageUrl, half } : null;
  const v = file.version;
  const fileName = meta.path.split(/[\\/]/).pop();
  const serverStatus = !server ? <span className="ai-warn">not chosen: saving asks where it goes</span>
    : server.error ? <span className="ai-warn" title={server.error}>{server.path} (cannot read: {server.error})</span>
    : <span className="mono truncate" title={server.path}>{server.path}{server.file && server.file.timestamp !== file.timestamp ? <span className="ai-warn"> · other timestamp</span> : null}</span>;
  const selectedProblems = selected ? (problems ?? []).filter((problem) => problem.target === selected.target && problem.index === selected.index) : [];

  const form = (() => {
    if (!selected) return <div className="empty-note">Select an entry in the list or on the map.</div>;
    if (file.kind === "precinct" && selected.target === "precinct") {
      const p = file.precincts[selected.index];
      if (!p) return null;
      const set = (change: Partial<MapPrecinct>, key: string) => setPrecinct(selected.index, change, key);
      return <>
        <Card title={p.name || "(no name)"} hint={p.id ? "in the client and the server file" : "ID 0: client only (names, music); the server file leaves it out"}>
          <div className="npcgen-rows">
            <Row label="Name" hint="Shown when entering the area"><TextInput value={p.name} onCommit={(name) => set({ name }, "name")} /></Row>
            {v >= 2 && <Row label="ID" hint="0 = client only: the server file leaves it out"><Int value={p.id} min={0} onCommit={(id) => set({ id }, "id")} /></Row>}
            <Row label="Priority" hint="Where areas overlap, the lowest number wins (the later area on a tie)"><Int value={p.priority} onCommit={(priority) => set({ priority }, "priority")} /></Row>
            {v >= 4 && <Row label="Instance" hint="The instance this area belongs to (a map folder can hold several)"><InstanceInput value={p.instance} maps={maps} onCommit={(instance) => set({ instance }, "instance")} /></Row>}
            {v >= 6 && <Row label="Domain" hint="Territory (domain.data ID); 0 = none"><Int value={p.domain} onCommit={(domain) => set({ domain }, "domain")} /></Row>}
          </div>
        </Card>
        <Card title="Return city" hint="Where “return to city” and reviving take players from this area">
          <div className="npcgen-rows">
            <Row label="Instance"><InstanceInput value={p.cityInstance} maps={maps} onCommit={(cityInstance) => set({ cityInstance }, "cityInstance")} /></Row>
            <Row label="Position"><VertInput value={p.city} onCommit={(city) => set({ city }, "city")} /></Row>
          </div>
        </Card>
        {v >= 7 ? <Card title="Level limits" hint="Player level counts as level + rebirths × 150. Outside min–max (+ rebirth count × 150) players are moved to the kick-out point. Needs a kick-out city above 0; 0 and 0 = no limit">
          <div className="npcgen-rows">
            <Row label="Rebirths"><Int value={p.rebirths} min={0} onCommit={(rebirths) => set({ rebirths }, "rebirths")} /></Row>
            <Row label="Minimum level" hint="0 = none"><Int value={p.levelMin} min={0} onCommit={(levelMin) => set({ levelMin }, "levelMin")} /></Row>
            <Row label="Maximum level" hint="0 = none"><Int value={p.levelMax} min={0} onCommit={(levelMax) => set({ levelMax }, "levelMax")} /></Row>
            <Row label="Kick-out city"><InstanceInput value={p.kickCity} maps={maps} onCommit={(kickCity) => set({ kickCity }, "kickCity")} /></Row>
            <Row label="Kick-out position"><VertInput value={p.kickPos} onCommit={(kickPos) => set({ kickPos }, "kickPos")} /></Row>
          </div>
        </Card> : <p className="muted small">This file is version {v}: level limits need version 7.</p>}
        <Card title="Outline" hint={`${p.points.length} points (x, height, z); the game closes the outline`}>
          <PointsTable points={p.points} onCommit={(points, key) => set({ points }, key)} />
        </Card>
        <Card title="Map marks" hint="Names shown on the client's map (client only)" actions={<button className="btn small" onClick={() => set({ marks: [...p.marks, { name: "New mark", pos: p.marks.length ? { ...p.marks[p.marks.length - 1].pos } : { ...p.city } }] }, `marks:${Date.now()}`)}><Plus size={12} /> Add</button>}>
          {p.marks.length ? <table className="dyn-table maps-marks">
            <thead><tr><th>Name</th><th>Position</th><th /></tr></thead>
            <tbody>{p.marks.map((mark, index) => <tr key={index}>
              <td><TextInput value={mark.name} onCommit={(name) => set({ marks: p.marks.map((entry, at) => at === index ? { ...entry, name } : entry) }, `mark:${index}:name`)} /></td>
              <td><VertInput value={mark.pos} onCommit={(pos) => set({ marks: p.marks.map((entry, at) => at === index ? { ...entry, pos } : entry) }, `mark:${index}:pos`)} /></td>
              <td><button className="icon-btn small danger" title="Remove" onClick={() => set({ marks: p.marks.filter((_, at) => at !== index) }, `marks:${Date.now()}`)}><Trash2 size={12} /></button></td>
            </tr>)}</tbody>
          </table> : <span className="muted small">No marks.</span>}
        </Card>
        <Card title="Sound and music" hint="Client only; paths below the client's music folder (sounds) or as written (music)">
          <div className="npcgen-rows">
            <Row label="Sound" hint="music\ + this path"><TextInput value={p.sound} onCommit={(sound) => set({ sound }, "sound")} /></Row>
            {v >= 3 && <Row label="Night sound" hint="music\ + this path"><TextInput value={p.soundNight} onCommit={(soundNight) => set({ soundNight }, "soundNight")} /></Row>}
            {v >= 8 && <Row label="Minimap" hint="Surfaces\ + this path"><TextInput value={p.minimap} onCommit={(minimap) => set({ minimap }, "minimap")} /></Row>}
            <Row label="Music interval"><Int value={p.musicInterval} onCommit={(musicInterval) => set({ musicInterval }, "musicInterval")} /></Row>
            <Row label="Music loop"><select className="task-form-select" value={p.musicLoop} onChange={(event) => set({ musicLoop: Number(event.target.value) }, "musicLoop")}>
              {[[0, "No loop"], [1, "Loop all"], [2, "Loop, skipping the first"]].concat([0, 1, 2].includes(p.musicLoop) ? [] : [[p.musicLoop, `${p.musicLoop}`]]).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
            </select></Row>
            <Row label="Music">
              <span className="maps-music">
                {p.music.map((music, index) => <span key={index} className="maps-music-row"><TextInput value={music} onCommit={(text) => set({ music: p.music.map((entry, at) => at === index ? text : entry) }, `music:${index}`)} /><button className="icon-btn small danger" title="Remove" onClick={() => set({ music: p.music.filter((_, at) => at !== index) }, `music:${Date.now()}`)}><Trash2 size={12} /></button></span>)}
                <button className="btn small" onClick={() => set({ music: [...p.music, ""] }, `music:${Date.now()}`)}><Plus size={12} /> Add music</button>
              </span>
            </Row>
          </div>
        </Card>
      </>;
    }
    if (file.kind === "region" && selected.target === "buff") {
      const buff = file.buffs[selected.index];
      if (!buff) return null;
      return <>
        <Card title={`Buff region ${selected.index + 1}`} hint="Server only (region.sev version 8): the client file cannot hold it">
          <div className="npcgen-rows">
            <Row label="Region template" hint="Buff area template ID (city_region::GetBuffArea)"><Int value={buff.template} onCommit={(template) => setBuff(selected.index, { template }, "template")} /></Row>
            <Row label="Monster" hint="Linked monster ID"><Int value={buff.monster} onCommit={(monster) => setBuff(selected.index, { monster }, "monster")} /></Row>
          </div>
        </Card>
        <Card title="Outline" hint={`${buff.points.length} points`}><PointsTable points={buff.points} onCommit={(points, key) => setBuff(selected.index, { points }, key)} /></Card>
      </>;
    }
    if (file.kind === "region" && selected.target === "item") {
      const item = file.items[selected.index];
      if (!item) return null;
      if (item.kind === "region") return <>
        <Card title={item.name || "(no name)"} hint="The server treats every region as a safe zone">
          <div className="npcgen-rows">
            <Row label="Name" hint="Client only"><TextInput value={item.name} onCommit={(name) => setItem(selected.index, { ...item, name }, "name")} /></Row>
            <Row label="Type" hint="Client only: the server file does not store it"><select className="task-form-select" value={item.regionType} onChange={(event) => setItem(selected.index, { ...item, regionType: Number(event.target.value) }, "type")}>
              {[[1, "Safe zone"], [0, "PK region"]].concat([0, 1].includes(item.regionType) ? [] : [[item.regionType, `${item.regionType}`]]).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
            </select></Row>
          </div>
        </Card>
        <Card title="Outline" hint={`${item.points.length} points`}><PointsTable points={item.points} onCommit={(points, key) => setItem(selected.index, { ...item, points }, key)} /></Card>
      </>;
      if (item.kind === "portal") {
        const number = file.items.slice(0, selected.index).filter((entry) => entry.kind === "portal").length;
        return <Card title={`Portal ${number + 1}`} hint="Players walking into the box (position ± size) move to the target. The client sends the server this portal's number">
          <div className="npcgen-rows">
            {v >= 3 && <Row label="From instance" hint="Only players in this instance use it"><InstanceInput value={item.instance} maps={maps} onCommit={(instance) => setItem(selected.index, { ...item, instance }, "instance")} /></Row>}
            <Row label="To instance"><InstanceInput value={item.target} maps={maps} onCommit={(target) => setItem(selected.index, { ...item, target }, "target")} /></Row>
            <Row label="Arrival position"><VertInput value={item.targetPos} onCommit={(targetPos) => setItem(selected.index, { ...item, targetPos }, "targetPos")} /></Row>
            <Row label="Box position" hint="The box's center"><VertInput value={item.pos} onCommit={(pos) => setItem(selected.index, { ...item, pos }, "pos")} /></Row>
            <Row label="Box half size" hint="The box reaches this far from its center on each axis (the editor's width, height and length halved)"><VertInput value={item.extents} onCommit={(extents) => setItem(selected.index, { ...item, extents }, "extents")} /></Row>
            {v >= 5 && <Row label="Level limit" hint="Players below it get a message instead"><Int value={item.level} onCommit={(level) => setItem(selected.index, { ...item, level }, "level")} /></Row>}
            {v >= 6 && <Row label="Line"><Int value={item.line} onCommit={(line) => setItem(selected.index, { ...item, line }, "line")} /></Row>}
            {v >= 7 && <Row label="Controller" hint="npcgen.data controller ID; -1 = none"><Int value={item.controller} onCommit={(controller) => setItem(selected.index, { ...item, controller }, "controller")} /></Row>}
          </div>
        </Card>;
      }
      return <Card title="Guild building" hint="Client only">
        <div className="npcgen-rows">
          <Row label="Template"><Int value={item.template} onCommit={(template) => setItem(selected.index, { ...item, template }, "template")} /></Row>
          <Row label="Width"><Int value={item.width} onCommit={(width) => setItem(selected.index, { ...item, width }, "width")} /></Row>
          <Row label="Height"><Int value={item.height} onCommit={(height) => setItem(selected.index, { ...item, height }, "height")} /></Row>
          <Row label="Position"><VertInput value={item.pos} onCommit={(pos) => setItem(selected.index, { ...item, pos }, "pos")} /></Row>
          <Row label="Direction"><VertInput value={item.dir} onCommit={(dir) => setItem(selected.index, { ...item, dir }, "dir")} /></Row>
          <Row label="Up"><VertInput value={item.up} onCommit={(up) => setItem(selected.index, { ...item, up }, "up")} /></Row>
        </div>
      </Card>;
    }
    return null;
  })();

  const tabCount = (key: RegionTab) => file.kind !== "region" ? 0 : key === "buff" ? file.buffs.length : file.items.filter((item) => item.kind === key).length;
  const canMove = !!selected && selected.target !== "buff";

  return <section className="path-data-pane maps-pane">
    <header className="path-data-head">
      <div>
        <h2>{file.kind === "precinct" ? "Precincts" : "Regions"} · {meta.folder}{folderMaps[0] ? ` (${folderMaps.map((map) => map.name).join(", ")})` : ""} {dirty && <span className="tag warn">unsaved</span>}</h2>
        <div className="path-data-file-line">
          <span className="path-data-location mono muted small truncate" title={meta.path}>{meta.path}</span>
          <span className="path-data-badge"><b>Version:</b> {v}</span>
          <span className="path-data-badge" title="Written to both files on save"><b>Timestamp:</b> {file.timestamp}</span>
        </div>
        <div className="path-data-file-line small maps-server-line"><Server size={12} /> <b>Server file:</b> {serverStatus} <button className="link" onClick={() => void chooseServer()}>{server ? "Change…" : "Choose…"}</button></div>
      </div>
      <button className="icon-btn" onClick={undo} disabled={busy || history.cursor <= 0} title="Undo (Ctrl+Z)"><Undo2 size={15} /></button>
      <button className="icon-btn" onClick={redo} disabled={busy || history.cursor + 1 >= history.states.length} title="Redo (Ctrl+Y)"><Redo2 size={15} /></button>
      <button className="btn" onClick={() => setProblemsOpen(true)} title="Problems (Ctrl+Shift+M)">{errors ? <CircleAlert size={13} className="danger-icon" /> : <TriangleAlert size={13} />} Problems{problems ? ` ${count(problems.length)}` : ""}</button>
      <button className="btn primary" onClick={saveCurrent} disabled={busy || (!dirty && server?.file?.timestamp === file.timestamp)} title={`Writes ${fileName} and the server's ${file.kind}.sev with one new timestamp (Ctrl+S)`}><Save size={13} /> Save</button>
      <label className="check nowrap"><input type="checkbox" checked={backup} onChange={(event) => { setBackup(event.target.checked); try { localStorage.setItem(BACKUP_KEY, event.target.checked ? "1" : "0"); } catch { /* optional */ } }} /> Backup</label>
      <button className="btn nowrap" onClick={() => void choose()} disabled={busy}><FolderOpen size={13} /> Open…</button>
    </header>
    {(error || note) && <div className={error ? "path-data-message error" : "path-data-message"} role={error ? "alert" : "status"}>
      {error ? <AlertTriangle size={14} /> : <Info size={14} />}{error || note} <button className="link" onClick={() => { setError(null); setNote(null); }}>Dismiss</button>
    </div>}
    {meta.siblingError && <div className="path-data-message"><Info size={14} /> The folder's other file cannot be shown on the map: {meta.siblingError}</div>}
    <div className="maps-body">
      <aside className="dyn-task-list">
        {file.kind === "region" && <div className="maps-tabs">{TABS.map((entry) => <button key={entry.key} className={"btn small" + (tab === entry.key ? " active" : "")} onClick={() => { setTab(entry.key); setSelected(null); }}>{entry.label} <span className="muted">{tabCount(entry.key)}</span></button>)}</div>}
        <div className="dyn-task-search"><Search size={13} /><input value={query} placeholder="Search" onChange={(event) => setQuery(event.target.value)} />{query && <button className="icon-btn small" onClick={() => setQuery("")}><X size={12} /></button>}</div>
        {instances.length > 1 && <div className="maps-instance-filter"><select className="task-form-select" value={instanceFilter ?? ""} onChange={(event) => setInstanceFilter(event.target.value === "" ? null : Number(event.target.value))}>
          <option value="">All instances</option>
          {instances.map((id) => <option key={id} value={id}>{id} · {mapName(id) ?? "not in instance.txt"}</option>)}
        </select></div>}
        <div className="dyn-task-rows" role="listbox">
          {rows.map((row) => {
            const isSelected = selected?.target === row.target && selected.index === row.index;
            const flag = flagged.get(`${row.target}:${row.index}`);
            return <button key={`${row.target}:${row.index}`} role="option" aria-selected={isSelected} className={"dyn-task-row maps-row" + (isSelected ? " selected" : "")} onClick={() => pick({ target: row.target, index: row.index })}>
              <span className="gshop-row-text"><span className="truncate">{row.label}</span><span className="muted small truncate">{row.detail}</span></span>
              {row.tag && <span className="tag">{row.tag}</span>}
              {flag === "error" ? <CircleAlert size={12} className="danger-icon" /> : flag ? <TriangleAlert size={12} className="ai-warn" /> : <span />}
            </button>;
          })}
          {!rows.length && <div className="empty-note">{query ? "Nothing matches." : "None."} {!query && <button className="link" onClick={addFirst}>Add one</button>}</div>}
        </div>
        <footer className="ai-list-foot">
          <span className="muted small">{count(rows.length)} shown</span>
          <span className="quiz-list-actions">
            <button className="icon-btn small" onClick={cloneSelected} disabled={busy || !selected} title="Clone (Ctrl+D)"><Copy size={13} /></button>
            <button className="icon-btn small" onClick={() => move(-1)} disabled={busy || !canMove} title="Move up (portals: their order is their number on the server)"><ArrowUp size={13} /></button>
            <button className="icon-btn small" onClick={() => move(1)} disabled={busy || !canMove} title="Move down"><ArrowDown size={13} /></button>
            <button className="icon-btn small danger" onClick={deleteSelected} disabled={busy || !selected} title="Delete (Del)"><Trash2 size={13} /></button>
          </span>
        </footer>
      </aside>
      <div className="dyn-task-form-scroll drops-form">
        {selectedProblems.map((problem, index) => <div key={index} className={"path-data-message" + (problem.severity === "error" ? " error" : "")}>{problem.severity === "error" ? <CircleAlert size={13} /> : <TriangleAlert size={13} />} {problem.message}</div>)}
        {form}
      </div>
      <MapPreview shapes={shapes} selected={selected} extras={extras} focus={focus} background={background} onImageError={() => setFailedImage(imageUrl)}
        onSelect={(selection) => { setSelected(selection); if (file.kind === "region") { if (selection.target === "buff") setTab("buff"); else { const item = file.items[selection.index]; if (item) setTab(item.kind); } } }}
        controls={<>
          {imageUrl ? <label className="check"><input type="checkbox" checked={showImage} onChange={(event) => setShowImage(event.target.checked)} /> Map image</label>
            : <span className="muted small">{mapGeneration === null ? "Set the game client folder in Settings to show the map image." : folderMaps.length ? "No map image for this map." : `“${meta.folder}” is not a map in the client's instance.txt.`}</span>}
          {meta.sibling && <label className="check"><input type="checkbox" checked={showSibling} onChange={(event) => setShowSibling(event.target.checked)} /> {meta.sibling.file.kind === "precinct" ? "Precincts" : "Regions"} (faded)</label>}
          <span className="maps-legend small">
            {file.kind === "precinct" ? <><span><i style={{ background: COLORS.precinct[0] }} />Area</span><span><i className="dashed" />Client only</span><span>◆ Return city</span><span>✕ Kick-out</span></>
              : <><span><i style={{ background: COLORS.safe }} />Safe zone</span><span><i style={{ background: COLORS.pk }} />PK</span><span><i style={{ background: COLORS.portal }} />Portal</span><span><i style={{ background: COLORS.buff }} />Buff region</span></>}
          </span>
          <span className="muted small">Wheel zooms, drag pans, double-click fits.</span>
        </>} />
    </div>
    {problemsOpen && <div className="modal-backdrop" onMouseDown={() => setProblemsOpen(false)}>
      <div className="modal npcgen-problems-dialog" role="dialog" aria-label="Problems" onMouseDown={(event) => event.stopPropagation()}>
        <header className="modal-head"><h3>Problems</h3><span className="muted small">{count(errors)} error{errors === 1 ? "" : "s"}</span><span className="spacer" /><button className="icon-btn" onClick={() => setProblemsOpen(false)} aria-label="Close"><X size={16} /></button></header>
        <div className="npcgen-problems-list">
          {!problems ? <div className="empty-note"><Loader2 size={14} className="spin" /> Checking…</div> : !problems.length ? <div className="empty-note">No problems.</div> : problems.map((problem, index) => <button key={index} className={"npcgen-problem " + problem.severity} onClick={() => {
            if (problem.index !== null && problem.target !== "file") {
              pick({ target: problem.target, index: problem.index });
              setQuery("");
              setInstanceFilter(null);
              if (file.kind === "region") setTab(problem.target === "buff" ? "buff" : (file.items[problem.index]?.kind ?? "region"));
            }
            setProblemsOpen(false);
          }}>
            {problem.severity === "error" ? <CircleAlert size={14} /> : problem.severity === "warning" ? <TriangleAlert size={14} /> : <Info size={14} />}
            <span>{problem.index !== null && <b>{problem.target === "precinct" ? `Precinct ${problem.index + 1}` : problem.target === "buff" ? `Buff region ${problem.index + 1}` : `Entry ${problem.index + 1}`}</b>} {problem.message}</span>
          </button>)}
        </div>
        <footer className="modal-foot muted small">From the client's and the server's readers (EL_Precinct/EL_Region) and the map editor's export.</footer>
      </div>
    </div>}
  </section>;
});
