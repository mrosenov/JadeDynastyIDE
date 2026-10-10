import { forwardRef, useCallback, useDeferredValue, useEffect, useImperativeHandle, useMemo, useRef, useState, type ReactNode } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { Archive, ArrowDown, ArrowUp, CircleAlert, Copy, Download, FileWarning, FolderOpen, Server, Gift, Loader2, Plus, Redo2, Save, Search, ShoppingCart, Trash2, TriangleAlert, Undo2, X } from "lucide-react";
import { closeGShopComparison, cloneGShopItem, compareGShop, copyGShop, deleteGShopItem, exportGShopJson, dynTaskLabels, editGShopCategories, getGShopItem, gshopLayouts, gshopProblems, gshopView, moveGShopItem, openGShop, pickEssence, redoGShop, resourceImageUrl, saveGShop, setGShopItem, undoGShop } from "../elements/api";
import { bytes, count } from "../elements/format";
import { formatDuration } from "../elements/time";
import type { GShopTextField, GShopComparison, GShopCopyResult, GShopCategoryOp, GShopItem, GShopLayout, GShopOtherField, GShopProblem, GShopSummary, GShopView } from "../elements/types";
import { GShopLayoutEditor } from "./GShopLayoutEditor";
import { GShopCompare } from "./GShopCompare";
import { GShopIconPicker } from "./GShopIconPicker";
import { GShopTextUpdate } from "./GShopTextUpdate";
import { NumberInput, TextInput } from "./DynTasksEditor";
import { GameText } from "./GameText";
import { ValuePicker } from "./ValuePicker";

export interface GShopEditorState {
  loaded: boolean;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  path: string | null;
  kind: string | null;
}

export interface GShopEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
  save: () => void;
  undo: () => void;
  redo: () => void;
  cloneSelected: () => void;
  deleteSelected: () => void;
  moveSelected: (step: -1 | 1) => void;
  openProblems: () => void;
  openCategories: () => void;
  /** Tools › Item layout: edit or create the layout the file is read with. */
  openLayout: () => void;
  openCompare: () => void;
  /** Tools › Update names / descriptions. */
  updateTexts: (field: GShopTextField) => void;
  exportJson: () => void;
  importJson: () => void;
}

interface Props {
  active: boolean;
  onStateChange: (state: GShopEditorState) => void;
  /** The open elements.data, for item names and the picker. */
  elementsPath?: string | null;
  /** Changes when the client folder changes; null without one (no icons). */
  resourceGeneration?: number | null;
}

const I32_MIN = -2147483648, I32_MAX = 2147483647;
const BACKUP_KEY = "jdide.gshop.backup";
/** The layout chosen per file (when several fit). */
const LAYOUT_KEY = "jdide.gshop.layoutFor";

function readLayoutChoices(): Record<string, string> {
  try {
    return JSON.parse(localStorage.getItem(LAYOUT_KEY) ?? "{}");
  } catch {
    return {};
  }
}

function rememberLayout(path: string, id: string) {
  const choices = readLayoutChoices();
  choices[path] = id;
  try { localStorage.setItem(LAYOUT_KEY, JSON.stringify(choices)); } catch { /* optional */ }
}

/** Bytes of a layout field (as the layout editor counts them). */
function fieldSize(field: GShopLayout["fields"][number]): number {
  switch (field.type) {
    case "u8": case "bool": return 1;
    case "u16": return 2;
    case "wstr": return (field.len ?? 0) * 2;
    case "str": case "bytes": return field.len ?? 0;
    case "group": return (field.count ?? 0) * (field.fields ?? []).reduce((total, child) => total + fieldSize(child), 0);
    default: return 4;
  }
}

/** A file no layout fits (from `NO_LAYOUT:`). */
interface NoLayout {
  path: string;
  recordSize: number | null;
  items: number;
  categories: number;
}
const FLAGS = ["New", "Recommended", "Promotion"];
const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const VALID_TYPES = ["Always", "Between two dates", "Every week", "Every month"];

/** The item mall shows prices in hundredths (100 = 1.00); the other shops as they are. */
const priceText = (kind: string, price: number) => (kind === "Item mall" ? (price / 100).toFixed(2) : String(price));
/** Descriptions keep line breaks as a literal \r (like the client's string tables). */
const toEditor = (text: string) => text.replace(/\\r|\r\n|\r|\n/g, "\n");
const fromEditor = (text: string) => text.replace(/\r\n|\r|\n/g, "\\r");
const bit = (value: number, index: number) => ((value >>> index) & 1) === 1;
const withBit = (value: number, index: number, on: boolean) => (on ? value | (1 << index) : value & ~(1 << index)) >>> 0;
const utcInput = (seconds: number) => (seconds > 0 ? new Date(seconds * 1000).toISOString().slice(0, 16) : "");
const fromUtcInput = (text: string) => (text ? Math.round(Date.parse(`${text}:00Z`) / 1000) : 0);
const dayTime = (seconds: number) => `${String(Math.floor(seconds / 3600)).padStart(2, "0")}:${String(Math.floor((seconds % 3600) / 60)).padStart(2, "0")}`;
const fromDayTime = (text: string) => { const [h, m] = text.split(":").map(Number); return (h || 0) * 3600 + (m || 0) * 60; };

function Card({ title, hint, wide, children }: { title: string; hint?: string; wide?: boolean; children: ReactNode }) {
  return <section className={"dyn-section npcgen-card" + (wide ? " wide" : "")}>
    <header><b>{title}</b>{hint && <span className="muted small">{hint}</span>}</header>
    <div className="npcgen-rows">{children}</div>
  </section>;
}

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return <>
    <span className="npcgen-label" title={hint}>{label}{hint && <span className="npcgen-hint">?</span>}</span>
    <span className="npcgen-value">{children}</span>
  </>;
}

function Check({ label, value, onChange, hint }: { label: string; value: boolean; onChange: (value: boolean) => void; hint?: string }) {
  return <label className="npcgen-flag" title={hint}><input type="checkbox" checked={value} onChange={(event) => onChange(event.target.checked)} /> {label}</label>;
}

function Icon({ url, size = 32 }: { url: string | null; size?: number }) {
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [url]);
  if (!url || failed) return <span className="gshop-icon empty" style={{ width: size, height: size }} />;
  return <img className="gshop-icon" src={url} width={size} height={size} alt="" onError={() => setFailed(true)} />;
}

function TextArea({ value, onCommit, rows = 4 }: { value: string; onCommit: (value: string) => void; rows?: number }) {
  const shown = toEditor(value);
  const [draft, setDraft] = useState(shown);
  useEffect(() => setDraft(shown), [shown]);
  return <textarea className="dyn-textarea" rows={rows} value={draft} onChange={(event) => setDraft(event.target.value)} onBlur={() => { const next = fromEditor(draft); if (next !== value) onCommit(next); }} />;
}

const RANGES: Record<string, [number, number]> = { u8: [0, 255], u16: [0, 65535], u32: [0, 4294967295], i32: [I32_MIN, I32_MAX] };

/** An editor for a field the layout gives no meaning, by its type. */
function OtherInput({ field, onCommit }: { field: GShopOtherField; onCommit: (value: GShopOtherField["value"]) => void }) {
  const [kind, size] = field.ty.split(":");
  if (kind === "bool") return <input type="checkbox" checked={!!field.value} onChange={(event) => onCommit(event.target.checked)} />;
  if (kind in RANGES) return <NumberInput min={RANGES[kind][0]} max={RANGES[kind][1]} value={Number(field.value)} onCommit={onCommit} />;
  if (kind === "f32") return <NumberInput float min={-1e30} max={1e30} value={Number(field.value)} onCommit={onCommit} />;
  if (kind === "bytes") return <span className="gshop-hex"><TextInput value={String(field.value)} onCommit={onCommit} /></span>;
  return <TextInput value={String(field.value)} max={Number(size) || undefined} onCommit={onCommit} />;
}

interface FormProps {
  item: GShopItem;
  view: GShopView;
  labels: Record<string, string>;
  elementsOpen: boolean;
  iconUrl: (path: string) => string | null;
  onCommit: (item: GShopItem, label: string) => void;
  pick: (current: number) => Promise<number | null>;
  pickIcon: (current: string) => Promise<string | null>;
}

function ItemLabel({ id, labels, elementsOpen }: { id: number; labels: Record<string, string>; elementsOpen: boolean }) {
  const label = labels[String(id)];
  if (label) return <span className="dyn-label" title={label}>{label.split(" › ").pop()}</span>;
  return elementsOpen && id ? <span className="npcgen-missing">not in elements.data</span> : null;
}

function ItemForm({ item, view, labels, elementsOpen, iconUrl, onCommit, pick, pickIcon }: FormProps) {
  const set = (change: Partial<GShopItem>, label: string) => onCommit({ ...item, ...change }, label);
  // Only what the file's layout stores is shown.
  const has = (...meanings: string[]) => meanings.some((meaning) => view.meanings.includes(meaning));
  const main = view.categories[item.mainType];
  const shop = view.kind;
  return <div className="dyn-form npcgen-form">
    <div className="npcgen-cards">
      {has("id", "num", "name", "icon", "local_id") && <Card title="Item">
        {has("id") && <Row label="Sells" hint="The item and how many per purchase"><span className="dyn-item-cell"><NumberInput value={item.id} onCommit={(id) => set({ id }, "Edit item")} /><button className="icon-btn small" title="Choose from elements.data" onClick={() => void pick(item.id).then((id) => { if (id !== null && id !== item.id) set({ id }, "Edit item"); })}><Search size={12} /></button><ItemLabel id={item.id} labels={labels} elementsOpen={elementsOpen} /></span></Row>}
        {has("num") && <Row label="Count"><NumberInput value={item.num} onCommit={(num) => set({ num }, "Edit count")} /></Row>}
        {has("name") && <Row label="Name" hint="Shown in the shop (at most 31 characters)"><TextInput value={item.name} max={31} onCommit={(name) => set({ name }, "Rename item")} /></Row>}
        {has("icon") && <Row label="Icon" hint="A picture in the client's surfaces.pck"><span className="gshop-icon-row"><Icon url={iconUrl(item.icon)} size={40} /><TextInput value={item.icon} onCommit={(icon) => set({ icon }, "Edit icon")} /><button className="btn small" onClick={() => void pickIcon(item.icon).then((icon) => { if (icon !== null) set({ icon }, "Edit icon"); })}>Choose…</button></span></Row>}
        {has("local_id") && <Row label="Local ID" hint="Used only for translation"><NumberInput min={I32_MIN} max={I32_MAX} value={item.localId} onCommit={(localId) => set({ localId }, "Edit local ID")} /></Row>}
      </Card>}
      {has("price", "time", "discount", "bonus", "props") && <Card title="Price" hint={shop === "Item mall" ? "In hundredths: 100 = 1.00" : undefined}>
        {has("price") && <Row label="Price" hint={shop === "Item mall" ? "Mall cash in hundredths" : shop === "Bonus shop" ? "Bonus points" : "Cross-server tokens"}><NumberInput value={item.price} onCommit={(price) => set({ price }, "Edit price")} /><span className="muted small">{priceText(shop, item.price)}</span></Row>}
        {has("time") && <Row label="Lasts" hint="Seconds; 0 = forever"><NumberInput value={item.time} onCommit={(time) => set({ time }, "Edit duration")} /><span className="muted small">{item.time ? formatDuration(item.time) : "forever"}</span></Row>}
        {has("discount") && <Row label="Discount" hint="Percent charged while one of its discount schemes is active (100 = full price)"><NumberInput min={I32_MIN} max={I32_MAX} value={item.discount} onCommit={(discount) => set({ discount }, "Edit discount")} /><span className="muted small">%</span></Row>}
        {has("bonus") && <Row label="Bonus" hint="Percent of the price given back as bonus"><NumberInput min={I32_MIN} max={I32_MAX} value={item.bonus} onCommit={(bonus) => set({ bonus }, "Edit bonus")} /><span className="muted small">%</span></Row>}
        {has("props") && <Row label="Flags"><span className="npcgen-inline">{FLAGS.map((flag, index) => <Check key={flag} label={flag} value={bit(item.props, index)} onChange={(on) => set({ props: withBit(item.props, index, on) }, `Edit ${flag.toLowerCase()} flag`)} />)}</span></Row>}
        {has("props") && <Row label="Sale schemes" hint="The server sells the item only while one of these schemes is active (gs config)"><span className="gshop-schemes">{Array.from({ length: 8 }, (_, index) => <Check key={index} label={String(index + 1)} value={bit(item.props, 16 + index)} onChange={(on) => set({ props: withBit(item.props, 16 + index, on) }, "Edit sale schemes")} />)}</span></Row>}
        {has("props") && <Row label="Discount schemes" hint="While one of these is active, the server charges price × discount %"><span className="gshop-schemes">{Array.from({ length: 8 }, (_, index) => <Check key={index} label={String(index + 1)} value={bit(item.props, 24 + index)} onChange={(on) => set({ props: withBit(item.props, 24 + index, on) }, "Edit discount schemes")} />)}</span></Row>}
      </Card>}
    </div>
    <div className="npcgen-cards">
      {has("main_type", "sub_type", "search_keys") && <Card title="Category">
        {has("main_type") && <Row label="Main"><select className="task-form-select" value={item.mainType} onChange={(event) => set({ mainType: Number(event.target.value), subType: 0 }, "Move to another category")}>{view.categories.map((category, index) => <option key={index} value={index}>{category.name || `Category ${index + 1}`}</option>)}{!main && <option value={item.mainType}>{item.mainType} · missing</option>}</select></Row>}
        {has("sub_type") && <Row label="Sub"><select className="task-form-select" value={item.subType} onChange={(event) => set({ subType: Number(event.target.value) }, "Move to another subcategory")}>{(main?.subs ?? []).map((sub, index) => <option key={index} value={index}>{sub || `Sub ${index + 1}`}</option>)}{!main?.subs[item.subType] && <option value={item.subType}>{item.subType} · missing</option>}</select></Row>}
        {has("search_keys") && <Row label="Keywords" hint="Search keywords, separated by commas (at most 63 characters)"><TextInput value={item.searchKeys} max={63} onCommit={(searchKeys) => set({ searchKeys }, "Edit keywords")} /></Row>}
      </Card>}
      {has("valid_type") && <Card title="On sale" hint="When the server sells it">
        <Row label="When"><select className="task-form-select" value={item.validType} onChange={(event) => set({ validType: Number(event.target.value), validStart: 0, validEnd: 0, validParam: 0 }, "Edit sale window")}>{VALID_TYPES.map((name, index) => <option key={index} value={index}>{name}</option>)}{!(item.validType in VALID_TYPES) && <option value={item.validType}>Type {item.validType}</option>}</select></Row>
        {item.validType === 1 && <>
          <Row label="From (UTC)"><Check label="" value={bit(item.validParam, 0)} onChange={(on) => set({ validParam: withBit(item.validParam, 0, on) }, "Edit sale start")} /><input type="datetime-local" className="dyn-input wide" value={utcInput(item.validStart)} disabled={!bit(item.validParam, 0)} onChange={(event) => set({ validStart: fromUtcInput(event.target.value) }, "Edit sale start")} /></Row>
          <Row label="Until (UTC)"><Check label="" value={bit(item.validParam, 1)} onChange={(on) => set({ validParam: withBit(item.validParam, 1, on) }, "Edit sale end")} /><input type="datetime-local" className="dyn-input wide" value={utcInput(item.validEnd)} disabled={!bit(item.validParam, 1)} onChange={(event) => set({ validEnd: fromUtcInput(event.target.value) }, "Edit sale end")} /></Row>
        </>}
        {(item.validType === 2 || item.validType === 3) && <>
          <Row label="From"><input type="time" className="dyn-input" value={dayTime(item.validStart)} onChange={(event) => set({ validStart: fromDayTime(event.target.value) }, "Edit sale start")} /></Row>
          <Row label="Until"><input type="time" className="dyn-input" value={dayTime(item.validEnd)} onChange={(event) => set({ validEnd: fromDayTime(event.target.value) }, "Edit sale end")} /></Row>
          {item.validType === 2
            ? <Row label="Days"><span className="npcgen-inline">{WEEKDAYS.map((day, index) => <Check key={day} label={day} value={bit(item.validParam, index)} onChange={(on) => set({ validParam: withBit(item.validParam, index, on) | 0 }, "Edit sale days")} />)}</span></Row>
            : <Row label="Days"><span className="gshop-days">{Array.from({ length: 31 }, (_, index) => <Check key={index} label={String(index + 1)} value={bit(item.validParam, index + 1)} onChange={(on) => set({ validParam: withBit(item.validParam, index + 1, on) | 0 }, "Edit sale days")} />)}</span></Row>}
        </>}
      </Card>}
    </div>
    {has("description") && <Card title="Description" hint="^RRGGBB colours a run; line breaks are kept" wide>
      <Row label="Text"><span className="gshop-text"><TextArea value={item.description} onCommit={(description) => set({ description }, "Edit description")} /><span className="gshop-preview"><GameText text={toEditor(item.description)} /></span></span></Row>
    </Card>}
    {has("has_present") && <Card title="Free gift" hint="Given with every purchase" wide>
      <Row label="Gift"><Check label="Gives a gift" value={item.hasPresent} onChange={(hasPresent) => set({ hasPresent }, "Edit gift")} /></Row>
      {item.hasPresent && <>
        <Row label="Item"><span className="dyn-item-cell"><NumberInput value={item.presentId} onCommit={(presentId) => set({ presentId }, "Edit gift item")} /><button className="icon-btn small" title="Choose from elements.data" onClick={() => void pick(item.presentId).then((presentId) => { if (presentId !== null && presentId !== item.presentId) set({ presentId }, "Edit gift item"); })}><Search size={12} /></button><ItemLabel id={item.presentId} labels={labels} elementsOpen={elementsOpen} /></span></Row>
        <Row label="Count"><NumberInput value={item.presentCount} onCommit={(presentCount) => set({ presentCount }, "Edit gift count")} /></Row>
        <Row label="Lasts" hint="Seconds; 0 = forever"><NumberInput value={item.presentTime} onCommit={(presentTime) => set({ presentTime }, "Edit gift duration")} /><span className="muted small">{item.presentTime ? formatDuration(item.presentTime) : "forever"}</span></Row>
        <Row label="Bound"><Check label="The gift is bound" value={item.presentBind} onChange={(presentBind) => set({ presentBind }, "Edit gift binding")} /></Row>
        <Row label="Name"><TextInput value={item.presentName} max={31} onCommit={(presentName) => set({ presentName }, "Rename gift")} /></Row>
        <Row label="Icon"><span className="gshop-icon-row"><Icon url={iconUrl(item.presentIcon)} size={32} /><TextInput value={item.presentIcon} onCommit={(presentIcon) => set({ presentIcon }, "Edit gift icon")} /><button className="btn small" onClick={() => void pickIcon(item.presentIcon).then((presentIcon) => { if (presentIcon !== null) set({ presentIcon }, "Edit gift icon"); })}>Choose…</button></span></Row>
        <Row label="Text"><TextArea rows={3} value={item.presentDescription} onCommit={(presentDescription) => set({ presentDescription }, "Edit gift text")} /></Row>
      </>}
    </Card>}
    {item.other.length > 0 && <Card title="Other fields" hint={`Fields of the layout ${view.layout.name} without a meaning; edited by their type`} wide>
      {item.other.map((field) => <Row key={field.path} label={field.path}><OtherInput field={field} onCommit={(value) => set({ other: item.other.map((entry) => (entry.path === field.path ? { ...entry, value } : entry)) }, `Edit ${field.path}`)} /><span className="muted small">{field.ty}</span></Row>)}
    </Card>}
  </div>;
}

// ── Categories ──

function CategoryEditor({ view, onEdit, onClose }: { view: GShopView; onEdit: (op: GShopCategoryOp) => Promise<boolean>; onClose: () => void }) {
  const [main, setMain] = useState(0);
  const [name, setName] = useState("");
  const [removing, setRemoving] = useState<{ sub: number; moveTo: number } | null>(null);
  const category = view.categories[main];
  const used = (sub: number) => view.items.filter((item) => item.mainType === main && item.subType === sub).length;
  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal gshop-categories" role="dialog" aria-label="Categories" onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head"><h3>Categories</h3><span className="muted small">Items keep their subcategory when subcategories move or are removed</span><span className="spacer" /><button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button></header>
      <div className="gshop-categories-body">
        <div className="gshop-mains">{view.categories.map((entry, index) => <button key={index} className={"dyn-task-row" + (index === main ? " selected" : "")} onClick={() => { setMain(index); setRemoving(null); }}><span className="mono muted">{entry.id}</span> {entry.name || <span className="muted">(no name)</span>} <span className="muted small">{count(view.items.filter((item) => item.mainType === index).length)}</span></button>)}</div>
        {category && <div className="gshop-subs">
          <div className="npcgen-rows">
            <span className="npcgen-label">Name</span><span className="npcgen-value"><TextInput value={category.name} max={63} onCommit={(value) => void onEdit({ op: "rename_main", main, name: value })} /></span>
          </div>
          <table className="dyn-table">
            <thead><tr><th>Subcategory</th><th>Items</th><th /></tr></thead>
            <tbody>{category.subs.map((sub, index) => <tr key={`${index}:${sub}`}>
              <td><TextInput value={sub} max={63} onCommit={(value) => void onEdit({ op: "rename_sub", main, sub: index, name: value })} /></td>
              <td className="mono">{count(used(index))}</td>
              <td><span className="task-form-row-actions">
                <button className="icon-btn small" disabled={index === 0} title="Move up" onClick={() => void onEdit({ op: "move_sub", main, sub: index, to: index - 1 })}><ArrowUp size={12} /></button>
                <button className="icon-btn small" disabled={index === category.subs.length - 1} title="Move down" onClick={() => void onEdit({ op: "move_sub", main, sub: index, to: index + 1 })}><ArrowDown size={12} /></button>
                <button className="icon-btn small danger" title="Remove" onClick={() => used(index) ? setRemoving({ sub: index, moveTo: index === 0 ? 1 : 0 }) : void onEdit({ op: "remove_sub", main, sub: index, moveTo: null })}><Trash2 size={12} /></button>
              </span></td>
            </tr>)}</tbody>
          </table>
          {removing && category.subs.length > 1 && <div className="path-data-message">
            {count(used(removing.sub))} item{used(removing.sub) === 1 ? "" : "s"} use “{category.subs[removing.sub]}”. Move them to <select className="task-form-select" value={removing.moveTo} onChange={(event) => setRemoving({ ...removing, moveTo: Number(event.target.value) })}>{category.subs.map((sub, index) => index !== removing.sub && <option key={index} value={index}>{sub}</option>)}</select>
            <button className="btn small danger" onClick={() => void onEdit({ op: "remove_sub", main, sub: removing.sub, moveTo: removing.moveTo }).then((done) => { if (done) setRemoving(null); })}>Remove and move</button>
            <button className="btn small" onClick={() => setRemoving(null)}>Cancel</button>
          </div>}
          {removing && category.subs.length <= 1 && <div className="path-data-message error">The only subcategory cannot be removed while items use it. <button className="link" onClick={() => setRemoving(null)}>OK</button></div>}
          <div className="dyn-table-foot"><input className="dyn-input wide" value={name} maxLength={63} placeholder="New subcategory name" onChange={(event) => setName(event.target.value)} /><button className="btn small" disabled={!name.trim()} onClick={() => void onEdit({ op: "add_sub", main, name: name.trim() }).then((done) => { if (done) setName(""); })}><Plus size={12} /> Add</button></div>
        </div>}
      </div>
    </div>
  </div>;
}

// ── The workspace ──

export const GShopEditor = forwardRef<GShopEditorHandle, Props>(function GShopEditor({ active, onStateChange, elementsPath = null, resourceGeneration = null }, ref) {
  const [view, setView] = useState<GShopView | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [item, setItem] = useState<GShopItem | null>(null);
  /** The category shown: all items, a main category, or one of its subcategories. */
  const [filter, setFilter] = useState<{ main: number | null; sub: number | null }>({ main: null, sub: null });
  const [query, setQuery] = useState("");
  const needle = useDeferredValue(query.trim().toLowerCase());
  const [labels, setLabels] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [saving, setSaving] = useState<{ changed: boolean } | null>(null);
  const [backup, setBackup] = useState(() => { try { return localStorage.getItem(BACKUP_KEY) !== "0"; } catch { return true; } });
  const [problems, setProblems] = useState<GShopProblem[] | null>(null);
  const [problemsOpen, setProblemsOpen] = useState(false);
  const [categoriesOpen, setCategoriesOpen] = useState(false);
  const [picking, setPicking] = useState<{ current: number; resolve: (value: number | null) => void } | null>(null);
  const savedOnce = useRef(false);
  /** A file no layout reads, and the layout editor (with the layout it starts from and the file it previews). */
  const [noLayout, setNoLayout] = useState<NoLayout | null>(null);
  const [layoutEditor, setLayoutEditor] = useState<{ start: GShopLayout; path: string | null; recordSize: number | null } | null>(null);
  const [layoutMenu, setLayoutMenu] = useState(false);
  const [compareOpen, setCompareOpen] = useState(false);
  const [comparison, setComparison] = useState<GShopComparison | null>(null);
  const [exporting, setExporting] = useState<"file" | "shown" | "selected" | null>(null);
  const [updatingTexts, setUpdatingTexts] = useState<GShopTextField | null>(null);

  useEffect(() => { gshopView().then((current) => { if (current) setView(current); }).catch(() => {}); }, []);
  useEffect(() => onStateChange({ loaded: !!view, dirty: !!view?.dirty, canUndo: !!view?.canUndo, canRedo: !!view?.canRedo, path: view?.path ?? null, kind: view?.kind ?? null }), [onStateChange, view]);

  // Names of the sold items from elements.data.
  useEffect(() => setLabels({}), [elementsPath]);
  useEffect(() => {
    if (!view || !elementsPath) return;
    const ids = [...new Set(view.items.map((entry) => entry.id))].filter((id) => id && !(String(id) in labels));
    if (item?.hasPresent && item.presentId && !(String(item.presentId) in labels)) ids.push(item.presentId);
    if (ids.length) dynTaskLabels(ids, []).then((found) => setLabels((current) => ({ ...current, ...found.elements }))).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [view, elementsPath, item?.presentId]);

  useEffect(() => {
    if (!view || selected === null) { setItem(null); return; }
    let cancelled = false;
    getGShopItem(selected).then((next) => { if (!cancelled) setItem(next); }).catch(() => { if (!cancelled) setItem(null); });
    return () => { cancelled = true; };
  }, [selected, view]);

  const iconUrl = useCallback((path: string) => (path && resourceGeneration !== null ? resourceImageUrl(resourceGeneration, path) : null), [resourceGeneration]);

  const run = useCallback(async (work: () => Promise<GShopView | void>) => {
    if (busy) return false;
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const next = await work();
      if (next) setView(next);
      return true;
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
      return false;
    } finally {
      setBusy(false);
    }
  }, [busy]);

  /** Opens a shop with the layout chosen for it before (or `layout`), noting files no layout reads. */
  const load = useCallback(async (path: string, layout?: string) => {
    if (view?.dirty && !window.confirm("The shop has unsaved changes. Open it again and discard them?")) return;
    setNoLayout(null);
    await run(async () => {
      let next: GShopView;
      try {
        next = await openGShop(path, layout ?? readLayoutChoices()[path] ?? null);
      } catch (problem) {
        const message = String(problem).replace(/^Error: /, "");
        if (!message.startsWith("NO_LAYOUT:")) throw problem;
        setNoLayout({ path, ...JSON.parse(message.slice("NO_LAYOUT:".length)) });
        return;
      }
      if (layout) rememberLayout(path, layout);
      setSelected(null);
      setComparison(null);
      setCompareOpen(false);
      void closeGShopComparison().catch(() => {});
      setFilter({ main: null, sub: null });
      setProblems(null);
      savedOnce.current = false;
      return next;
    });
  }, [run, view?.dirty]);
  const choose = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, defaultPath: view?.path, title: "Open a shop file (gshop.data, gshop1.data or gshop2.data)", filters: [{ name: "gshop data", extensions: ["data"] }] });
    if (typeof picked === "string") await load(picked);
  }, [load, view?.path]);
  const commit = useCallback((next: GShopItem, label: string) => {
    if (selected === null) return;
    void run(() => setGShopItem(selected, next, label));
  }, [run, selected]);
  const pick = useCallback((current: number) => new Promise<number | null>((resolve) => setPicking({ current, resolve })), []);
  const [iconPicking, setIconPicking] = useState<{ current: string; resolve: (value: string | null) => void } | null>(null);
  const pickIcon = useCallback((current: string) => new Promise<string | null>((resolve) => setIconPicking({ current, resolve })), []);
  /** Icons the shop uses (lowercase) and how often, for the icon picker. */
  const usedIcons = useMemo(() => {
    const out = new Map<string, number>();
    for (const entry of view?.items ?? []) if (entry.icon) out.set(entry.icon.toLowerCase(), (out.get(entry.icon.toLowerCase()) ?? 0) + 1);
    return out;
  }, [view]);
  const pickSearch = useMemo(() => picking ? (text: string, page: number) => pickEssence("item", text, page, picking.current || null) : undefined, [picking]);

  const undo = useCallback(() => { if (view?.canUndo) void run(undoGShop); }, [run, view?.canUndo]);
  const redo = useCallback(() => { if (view?.canRedo) void run(redoGShop); }, [run, view?.canRedo]);
  const anyWindow = problemsOpen || categoriesOpen || compareOpen || !!exporting || !!updatingTexts || !!layoutEditor || !!iconPicking || !!saving;
  const cloneSelected = useCallback(() => {
    if (selected === null || anyWindow) return;
    void run(async () => { const result = await cloneGShopItem(selected); setSelected(result.index); return result.view; });
  }, [anyWindow, run, selected]);
  const deleteSelected = useCallback(() => {
    if (selected === null || anyWindow || !view) return;
    if (!window.confirm(`Delete item ${selected + 1} (${view.items[selected]?.name || "no name"})? Undo brings it back. The server finds items by position, so give it the saved file too.`)) return;
    void run(async () => { const next = await deleteGShopItem(selected); setSelected(next.items.length ? Math.min(selected, next.items.length - 1) : null); return next; });
  }, [anyWindow, run, selected, view]);

  // The shown items, in file order (the order purchases go by).
  const rows = useMemo(() => (view?.items ?? []).filter((entry) => {
    if (filter.main !== null && entry.mainType !== filter.main) return false;
    if (filter.sub !== null && entry.subType !== filter.sub) return false;
    if (!needle) return true;
    return entry.name.toLowerCase().includes(needle) || String(entry.id) === needle || String(entry.index + 1) === needle || (labels[String(entry.id)] ?? "").toLowerCase().includes(needle);
  }), [filter, labels, needle, view]);
  /** Moves the selected item before or after its neighbour in the shown list. */
  const moveSelected = useCallback((step: -1 | 1) => {
    if (selected === null || anyWindow) return;
    const at = rows.findIndex((entry) => entry.index === selected);
    const neighbour = rows[at + step];
    if (at < 0 || !neighbour) return;
    void run(async () => { const next = await moveGShopItem(selected, neighbour.index); setSelected(neighbour.index); return next; });
  }, [anyWindow, rows, run, selected]);

  const checkProblems = useCallback(async () => {
    setProblems(null);
    try {
      setProblems(await gshopProblems());
    } catch (problem) {
      setProblemsOpen(false);
      setError(String(problem).replace(/^Error: /, ""));
    }
  }, []);
  const openProblems = useCallback(() => { if (!view) return; setProblemsOpen(true); void checkProblems(); }, [checkProblems, view]);
  const editCategories = useCallback((op: GShopCategoryOp) => run(() => editGShopCategories(op)), [run]);
  /** The layout editor, starting from the open file's layout or the closest one for a file no layout reads. */
  const openLayout = useCallback(async () => {
    try {
      const layouts = await gshopLayouts();
      if (view) {
        const current = layouts.find((entry) => entry.id === view.layout.id) ?? layouts[layouts.length - 1];
        setLayoutEditor({ start: current, path: null, recordSize: view.recordSize });
      } else if (noLayout) {
        const size = (entry: GShopLayout) => entry.fields.reduce((total, field) => total + fieldSize(field), 0);
        const closest = [...layouts].sort((a, b) => Math.abs(size(a) - (noLayout.recordSize ?? 0)) - Math.abs(size(b) - (noLayout.recordSize ?? 0)))[0];
        setLayoutEditor({ start: closest, path: noLayout.path, recordSize: noLayout.recordSize });
      }
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    }
  }, [noLayout, view]);

  const openCompare = useCallback(() => {
    if (!view) return;
    setCompareOpen(true);
    // Edits since the last comparison change it: compare again.
    if (comparison) compareGShop(null).then(setComparison).catch(() => setComparison(null));
  }, [comparison, view]);
  const chooseCompare = useCallback(async (json = false) => {
    const picked = await open({ multiple: false, directory: false, defaultPath: comparison?.path ?? view?.path, title: json ? "Import shop JSON" : "Compare with another shop file or a JSON export", filters: [json ? { name: "Shop JSON export", extensions: ["json"] } : { name: "gshop data or JSON export", extensions: ["data", "json"] }] });
    if (typeof picked !== "string") return false;
    return run(async () => { setComparison(await compareGShop(picked)); });
  }, [comparison?.path, run, view?.path]);
  const importJson = useCallback(async () => {
    if (view && await chooseCompare(true)) setCompareOpen(true);
  }, [chooseCompare, view]);
  const copyCompared = useCallback(async (picks: number[]) => {
    let report: GShopCopyResult["report"] | null = null;
    await run(async () => {
      const result = await copyGShop(picks);
      report = result.report;
      setComparison(await compareGShop(null));
      return result.view;
    });
    const done = report as GShopCopyResult["report"] | null;
    if (!done) return;
    const parts = [done.added && `added ${done.added}`, done.replaced && `replaced ${done.replaced}`, done.subcategories.length && `added the subcategor${done.subcategories.length === 1 ? "y" : "ies"} ${done.subcategories.join(", ")}`].filter(Boolean);
    setNote(`${parts.join(", ").replace(/^./, (first) => first.toUpperCase()) || "Nothing changed"} (one undo step). Save to keep them.`);
  }, [run]);
  const showItem = useCallback((index: number) => {
    setCompareOpen(false);
    setFilter({ main: null, sub: null });
    setQuery("");
    setSelected(index);
  }, []);

  const writeOut = useCallback(async (replaceChanged: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const report = await saveGShop(backup, replaceChanged);
      savedOnce.current = true;
      setSaving(null);
      setNote(`Saved ${report.path} (timestamp ${report.timestamp}).${report.backup ? ` Backup: ${report.backup}.` : ""} Copy it to the server's config folder too and restart the server.`);
      setView(await gshopView());
    } catch (problem) {
      const message = String(problem).replace(/^Error: /, "");
      if (message.startsWith("CHANGED_ON_DISK")) setSaving({ changed: true });
      else setError(message);
    } finally {
      setBusy(false);
    }
  }, [backup]);
  const saveCurrent = useCallback(() => { if (!view) return; if (savedOnce.current) void writeOut(false); else setSaving({ changed: false }); }, [view, writeOut]);

  useImperativeHandle(ref, () => ({ choose: () => void choose(), openPath: (path) => void load(path), save: saveCurrent, undo, redo, cloneSelected, deleteSelected, moveSelected, openProblems, openCategories: () => { if (view) setCategoriesOpen(true); }, openLayout: () => void openLayout(), openCompare, updateTexts: (field) => { if (view) setUpdatingTexts(field); }, exportJson: () => { if (view) setExporting("file"); }, importJson: () => void importJson() }), [openCompare, importJson, openLayout, choose, cloneSelected, deleteSelected, load, moveSelected, openProblems, redo, saveCurrent, undo, view]);

  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      const mod = event.ctrlKey || event.metaKey;
      const typing = event.target instanceof HTMLElement && /^(INPUT|TEXTAREA|SELECT)$/.test(event.target.tagName);
      const key = event.key.toLowerCase();
      if (event.key === "Escape" && (problemsOpen || categoriesOpen || compareOpen || exporting)) {
        event.preventDefault();
        setProblemsOpen(false);
        setCategoriesOpen(false);
        setCompareOpen(false);
        setExporting(null);
        return;
      }
      if (mod && event.shiftKey && key === "m") {
        event.preventDefault();
        event.stopImmediatePropagation();
        openProblems();
        return;
      }
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
      } else if (event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown") && !typing) {
        event.preventDefault();
        moveSelected(event.key === "ArrowUp" ? -1 : 1);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [active, categoriesOpen, compareOpen, exporting, choose, cloneSelected, deleteSelected, moveSelected, openProblems, problemsOpen, redo, saveCurrent, undo]);

  const layoutWindow = layoutEditor && <GShopLayoutEditor start={layoutEditor.start} path={layoutEditor.path} recordSize={layoutEditor.recordSize}
    onSaved={(layout) => { const path = layoutEditor.path ?? view?.path; setLayoutEditor(null); if (path) void load(path, layout.id); }}
    onDeleted={() => { const path = layoutEditor.path ?? view?.path; setLayoutEditor(null); if (path) void load(path); }}
    onClose={() => setLayoutEditor(null)} />;

  if (!view) return <section className="dyn-tasks-pane empty">
    {layoutWindow}
    <div className="drop-card tasks-empty">
      {noLayout && <div className="path-data-message error gshop-no-layout">
        <b>No item layout reads {noLayout.path.split(/[\\/]/).pop()}.</b> It has {count(noLayout.items)} items{noLayout.recordSize ? ` of ${count(noLayout.recordSize)} bytes each` : ""} and {count(noLayout.categories)} main {noLayout.categories === 1 ? "category" : "categories"}. A newer client may have added fields: describe its items in a layout and the editor can read it.
        <div><button className="btn primary" onClick={() => void openLayout()}>Create a layout…</button></div>
      </div>}
      <ShoppingCart size={34} />
      <h2>Open a shop file</h2>
      <p className="muted">gshop.data is the item mall, gshop1.data the bonus shop and gshop2.data the cross-server shop. The server needs exactly the same file as the client, so after saving, copy it to the server's config folder too.</p>
      <button className="btn primary" onClick={() => void choose()} disabled={busy}>{busy ? <Loader2 size={14} className="spin" /> : <FolderOpen size={14} />} Choose a gshop file…</button>
      {error && <div className="path-data-message error">{error}</div>}
    </div>
  </section>;

  const countIn = (main: number | null, sub: number | null) => view.items.filter((entry) => (main === null || entry.mainType === main) && (sub === null || entry.subType === sub)).length;
  const fileName = (path: string) => path.split(/[\\/]/).slice(-3).join("/");
  const summaryRow = (entry: GShopSummary) => <button key={entry.index} role="option" aria-selected={entry.index === selected} className={"dyn-task-row gshop-row" + (entry.index === selected ? " selected" : "")} onClick={() => setSelected(entry.index)}>
    <span className="mono muted">{entry.index + 1}</span>
    <Icon url={iconUrl(entry.icon)} size={28} />
    <span className="gshop-row-text"><span className="truncate">{entry.name || <span className="muted">(no name)</span>}{entry.num > 1 && <span className="muted"> ×{entry.num}</span>}</span><span className="muted small truncate"><span className="mono">{entry.id}</span>{labels[String(entry.id)] ? ` · ${labels[String(entry.id)].split(" › ").pop()}` : ""}</span></span>
    <span className="gshop-row-price"><b>{priceText(view.kind, entry.price)}</b>{entry.hasPresent && <Gift size={12} className="muted" />}{entry.props & 1 ? <span className="tag ok">new</span> : null}</span>
    {entry.changed ? <span className="changed-dot" title="Changed" /> : <span />}
  </button>;

  return <section className="dyn-tasks-pane" aria-busy={busy}>
    <header className="tasks-head">
      <div><h2>{view.kind} {view.dirty && <span className="tag warn">unsaved</span>}</h2><div className="tasks-file-line">
        <span className="mono truncate" title={view.path}>{fileName(view.path)}</span>
        <span className="path-data-badge"><b>Items:</b> {count(view.items.length)}</span>
        <span className="path-data-badge" title="The client compares it with the server's: both need the same file"><b>Timestamp:</b> {view.timestamp}</span>
        <span className="gshop-layout-pick">
          <button className="path-data-badge" title={`Items are read with this layout (${count(view.layout.size)} bytes each). Click to switch or edit it.`} onClick={() => setLayoutMenu((open) => !open)}><b>Layout:</b> {view.layout.name}</button>
          {layoutMenu && <span className="gshop-layout-menu" onMouseLeave={() => setLayoutMenu(false)}>
            {view.alternatives.map((entry) => <button key={entry.id} className="btn small" onClick={() => { setLayoutMenu(false); void load(view.path, entry.id); }}>Read with {entry.name}</button>)}
            <button className="btn small" onClick={() => { setLayoutMenu(false); void openLayout(); }}>Edit layout…</button>
          </span>}
        </span>
        {view.recordSize && <span className="path-data-badge" title="Bytes per item in this file"><b>Record:</b> {bytes(view.recordSize)}</span>}
      </div></div>
      <button className="btn" onClick={undo} disabled={!view.canUndo || busy} title="Undo (Ctrl+Z)"><Undo2 size={14} /></button>
      <button className="btn" onClick={redo} disabled={!view.canRedo || busy} title="Redo (Ctrl+Y)"><Redo2 size={14} /></button>
      <button className="btn" onClick={openProblems} title="Check problems (Ctrl+Shift+M)"><CircleAlert size={14} /> Problems</button>
      <button className="btn" onClick={() => void choose()} disabled={busy}><FolderOpen size={14} /> Open…</button>
      <button className="btn primary" onClick={saveCurrent} disabled={busy}><Save size={14} /> Save</button>
    </header>
    {error && <div className="path-data-message error">{error} <button className="link" onClick={() => setError(null)}>Dismiss</button></div>}
    {note && <div className="path-data-message ok">{note} <button className="link" onClick={() => setNote(null)}>Dismiss</button></div>}
    <div className="gshop-body">
      <aside className="gshop-tree">
        <button className={"dyn-task-row" + (filter.main === null ? " selected" : "")} onClick={() => setFilter({ main: null, sub: null })}><b>All items</b> <span className="muted small">{count(view.items.length)}</span></button>
        {view.categories.map((category, main) => <div key={main}>
          <button className={"dyn-task-row" + (filter.main === main && filter.sub === null ? " selected" : "")} onClick={() => setFilter({ main, sub: null })}><span>{category.name || `Category ${main + 1}`}</span> <span className="muted small">{count(countIn(main, null))}</span></button>
          {filter.main === main && category.subs.map((sub, index) => <button key={index} className={"dyn-task-row gshop-sub" + (filter.sub === index ? " selected" : "")} onClick={() => setFilter({ main, sub: index })}><span>{sub || `Sub ${index + 1}`}</span> <span className="muted small">{count(countIn(main, index))}</span></button>)}
        </div>)}
        <footer><button className="btn small" onClick={() => setCategoriesOpen(true)}>Edit categories…</button></footer>
      </aside>
      <aside className="dyn-task-list gshop-list">
        <div className="dyn-task-search"><Search size={13} /><input value={query} placeholder="Name, item ID or position" onChange={(event) => setQuery(event.target.value)} />{query && <button className="icon-btn small" onClick={() => setQuery("")}><X size={12} /></button>}</div>
        <div className="dyn-task-rows" role="listbox">
          {rows.map(summaryRow)}
          {!rows.length && <div className="empty-note">Nothing here.</div>}
        </div>
        <footer>
          <button className="btn small" onClick={cloneSelected} disabled={selected === null || busy} title="Copy below (Ctrl+D); new items are made by copying"><Copy size={13} /> Clone</button>
          <button className="btn small danger" onClick={deleteSelected} disabled={selected === null || busy} title="Delete (Del)"><Trash2 size={13} /></button>
          <button className="icon-btn small" onClick={() => moveSelected(-1)} disabled={selected === null || busy} title="Move before the previous item shown (Alt+↑)"><ArrowUp size={13} /></button>
          <button className="icon-btn small" onClick={() => moveSelected(1)} disabled={selected === null || busy} title="Move after the next item shown (Alt+↓)"><ArrowDown size={13} /></button>
          <span className="spacer" />
          <span className="muted small">{count(rows.length)} / {count(view.items.length)}</span>
        </footer>
      </aside>
      <div className="dyn-task-form-scroll">
        {selected === null ? <div className="empty-note">Select an item. The list is in file order: the server finds a bought item by its position, so it needs exactly the saved file.</div>
          : item ? <><div className="dyn-task-title"><h3>Item {selected + 1}</h3>{item.name && <span className="muted">{item.name}</span>}</div>
            <ItemForm key={selected} item={item} view={view} labels={labels} elementsOpen={!!elementsPath} iconUrl={iconUrl} pick={pick} pickIcon={pickIcon} onCommit={commit} /></>
          : <div className="empty-note">Loading…</div>}
      </div>
    </div>
    {layoutWindow}
    {updatingTexts && <GShopTextUpdate field={updatingTexts} view={view} shownItems={rows.map((entry) => entry.index)} onClose={() => setUpdatingTexts(null)}
      onApplied={(next, changed) => { setView(next); setUpdatingTexts(null); setNote(`Updated the ${updatingTexts === "name" ? "names" : "descriptions"} of ${count(changed)} item${changed === 1 ? "" : "s"} (one undo step). Save to keep them.`); if (selected !== null) getGShopItem(selected).then(setItem).catch(() => {}); }} />}
    {iconPicking && <GShopIconPicker current={iconPicking.current} used={usedIcons} iconUrl={iconUrl} onPick={(path) => { iconPicking.resolve(path); setIconPicking(null); }} onClose={() => { iconPicking.resolve(null); setIconPicking(null); }} />}
    {compareOpen && <div className="modal-backdrop" onMouseDown={() => setCompareOpen(false)}>
      <div className="modal npcgen-nearby-dialog" role="dialog" aria-label="Compare" onMouseDown={(event) => event.stopPropagation()}>
        <header className="modal-head"><h3>Compare with another shop</h3><span className="spacer" /><button className="icon-btn" onClick={() => setCompareOpen(false)} aria-label="Close" title="Close (Esc); the comparison stays"><X size={16} /></button></header>
        <GShopCompare comparison={comparison} kind={view.kind} busy={busy} labels={labels} price={(value) => priceText(view.kind, value)} onChoose={() => void chooseCompare()} onCopy={(picks) => void copyCompared(picks)} onShow={showItem} />
      </div>
    </div>}
    {exporting && (() => {
      const picks = exporting === "file" ? null : exporting === "selected" ? (selected === null ? [] : [selected]) : rows.map((entry) => entry.index);
      const write = async () => {
        const base = view.path.replace(/\.data$/i, "");
        const target = await save({ defaultPath: `${base}${exporting === "file" ? "" : exporting === "selected" ? `_item${(selected ?? 0) + 1}` : "_items"}.json`, title: "Export shop JSON", filters: [{ name: "JSON", extensions: ["json"] }] });
        if (!target) return;
        try {
          const counts = await exportGShopJson(target, picks);
          setExporting(null);
          setNote(`Exported ${count(counts.items)} item${counts.items === 1 ? "" : "s"} and the categories to ${target}.`);
        } catch (problem) {
          setError(String(problem).replace(/^Error: /, ""));
        }
      };
      return <div className="modal-backdrop" onMouseDown={() => setExporting(null)}>
        <div className="modal dyn-save-dialog" role="dialog" aria-label="Export JSON" onMouseDown={(event) => event.stopPropagation()}>
          <h3>Export JSON</h3>
          <div className="npcgen-stack npcgen-export-scope">
            <label className="npcgen-flag"><input type="radio" checked={exporting === "file"} onChange={() => setExporting("file")} /> The whole shop <span className="muted small">({count(view.items.length)} items)</span></label>
            <label className="npcgen-flag"><input type="radio" checked={exporting === "shown"} disabled={!rows.length} onChange={() => setExporting("shown")} /> The items shown in the list <span className="muted small">({count(rows.length)})</span></label>
            <label className="npcgen-flag"><input type="radio" checked={exporting === "selected"} disabled={selected === null} onChange={() => setExporting("selected")} /> The selected item {selected !== null && <span className="muted small">({view.items[selected]?.name || `item ${selected + 1}`})</span>}</label>
          </div>
          <p className="muted small">The JSON keeps every field the layout reads and all categories. Import it with Tools › Import JSON… (in this or another shop): it opens like a comparison, so you choose what to copy.</p>
          <footer><span className="spacer" /><button className="btn" onClick={() => setExporting(null)}>Cancel</button><button className="btn primary" disabled={!!picks && !picks.length} onClick={() => void write()}><Download size={14} /> Export…</button></footer>
        </div>
      </div>;
    })()}
    {categoriesOpen && <CategoryEditor view={view} onEdit={editCategories} onClose={() => setCategoriesOpen(false)} />}
    {problemsOpen && (() => {
      const errors = (problems ?? []).filter((problem) => problem.severity === "error").length;
      const warnings = (problems ?? []).length - errors;
      return <div className="modal-backdrop" onMouseDown={() => setProblemsOpen(false)}>
        <div className="modal npcgen-problems-dialog" role="dialog" aria-label="Problems" onMouseDown={(event) => event.stopPropagation()}>
          <header className="modal-head"><h3>Problems</h3>{problems && <span className="muted small">{count(errors)} error{errors === 1 ? "" : "s"} · {count(warnings)} warning{warnings === 1 ? "" : "s"}</span>}<span className="spacer" /><button className="btn small" onClick={() => void checkProblems()} disabled={!problems}>Check again</button><button className="icon-btn" onClick={() => setProblemsOpen(false)} aria-label="Close"><X size={16} /></button></header>
          <div className="npcgen-problems-list">
            {!problems ? <div className="empty-note"><Loader2 size={14} className="spin" /> Checking…</div>
              : problems.length ? [...problems].sort((a, b) => (a.severity === b.severity ? (a.index ?? -1) - (b.index ?? -1) : a.severity === "error" ? -1 : 1)).map((problem, index) => <button key={index} className={`npcgen-problem ${problem.severity}`} onClick={() => { if (problem.index === null) return; setProblemsOpen(false); setFilter({ main: null, sub: null }); setSelected(problem.index); }}>
                {problem.severity === "error" ? <CircleAlert size={14} /> : <TriangleAlert size={14} />}
                <span className="npcgen-problem-item">{problem.index === null ? "Shop" : `Item ${problem.index + 1}`}</span>
                <span className="npcgen-problem-name truncate">{problem.index !== null ? view.items[problem.index]?.name : ""}</span>
                <span className="npcgen-problem-message">{problem.message}</span>
              </button>)
              : <div className="empty-note">No problems found.</div>}
          </div>
          <footer className="modal-foot muted small">{elementsPath ? "Items and gifts are checked against the open elements.data." : "Open the server's elements.data to check items and gifts too."}</footer>
        </div>
      </div>;
    })()}
    {picking && pickSearch && <ValuePicker search={pickSearch} onApply={async (value) => { picking.resolve(Number(value)); return null; }} onClose={() => { picking.resolve(null); setPicking(null); }} />}
    {saving && (() => {
      const changedItems = view.items.filter((entry) => entry.changed).length;
      const name = view.path.split(/[\\/]/).pop() ?? "gshop.data";
      return <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !busy && setSaving(null)}>
        <div className="modal save-dialog" role="dialog" aria-label="Save the shop">
          <header className="modal-head"><Save size={16} className="accent-icon" /><h3 className="truncate">Save {name}</h3><span className="spacer" /><button className="icon-btn" onClick={() => setSaving(null)} disabled={busy} aria-label="Close"><X size={16} /></button></header>
          <div className="save-body">
            <div className="save-target"><div className="save-target-path mono truncate" title={view.path}>{view.path}</div></div>
            <div className="save-facts">
              <span className="tag">{view.kind}</span>
              <span className="muted small">{count(view.items.length)} items · read with {view.layout.name}</span>
              <span className="spacer" />
              <span className="save-changes small">{changedItems > 0 ? <span className="chg changed">{count(changedItems)} changed item{changedItems === 1 ? "" : "s"}</span> : view.dirty ? <span className="chg changed">Unsaved changes</span> : <span className="muted">No changes since the last save</span>}</span>
            </div>
            {saving.changed && <div className="save-note danger"><FileWarning size={15} /><div><b>The file was changed by another program</b> since JD IDE read it. Saving replaces those changes with what is open here.</div></div>}
            <div className="save-note muted"><Server size={15} /><div>Saving gives the shop a new timestamp, and the client only opens the mall when the server has the same one. <b>Copy the saved file to the server's config folder</b> (e.g. <span className="mono">gamed/config/{name}</span>) and restart the server.</div></div>
            <label className="save-backup">
              <input type="checkbox" checked={backup} disabled={busy} onChange={(event) => { setBackup(event.target.checked); try { localStorage.setItem(BACKUP_KEY, event.target.checked ? "1" : "0"); } catch { /* optional */ } }} />
              <Archive size={14} />
              <span>Keep a backup of the replaced file<span className="muted small save-backup-name"> · jdide_backups\{name}_<i>date-time</i>.7z, once per session</span></span>
            </label>
          </div>
          <footer className="modal-foot">
            <span className="muted small">Undo keeps working after saving.</span>
            <span className="spacer" />
            <button className="btn" onClick={() => setSaving(null)} disabled={busy}>Cancel</button>
            <button className="btn primary" onClick={() => void writeOut(saving.changed)} disabled={busy} autoFocus>{busy ? <Loader2 size={14} className="spin" /> : <Save size={14} />} {busy ? "Saving…" : saving.changed ? "Replace anyway" : "Save"}</button>
          </footer>
        </div>
      </div>;
    })()}
  </section>;
});
