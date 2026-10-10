import { Fragment, useEffect, useMemo, useState } from "react";
import { ArrowDown, ArrowUp, CircleAlert, Loader2, Plus, Save, Trash2, X } from "lucide-react";
import { deleteGShopLayout, previewGShopLayout, saveGShopLayout } from "../elements/api";
import { count } from "../elements/format";
import type { GShopField, GShopFieldType, GShopLayout, GShopLayoutPreview } from "../elements/types";

/** The meanings the editor knows, and which field types can carry them. */
const MEANINGS: [string, string, "int" | "flag" | "wstr" | "str"][] = [
  ["id", "Item sold", "int"], ["num", "Count", "int"], ["icon", "Icon path", "str"], ["price", "Price", "int"], ["time", "Duration", "int"],
  ["discount", "Discount %", "int"], ["bonus", "Bonus %", "int"], ["props", "Flags and schemes", "int"], ["main_type", "Main category", "int"],
  ["sub_type", "Subcategory", "int"], ["local_id", "Local ID", "int"], ["description", "Description", "wstr"], ["name", "Name", "wstr"],
  ["has_present", "Has a gift", "flag"], ["present_name", "Gift name", "wstr"], ["present_id", "Gift item", "int"], ["present_count", "Gift count", "int"],
  ["present_time", "Gift duration", "int"], ["present_icon", "Gift icon", "str"], ["present_bind", "Gift bound", "flag"],
  ["present_description", "Gift text", "wstr"], ["valid_type", "Sale window type", "int"], ["valid_start", "Sale start", "int"],
  ["valid_end", "Sale end", "int"], ["valid_param", "Sale days", "int"], ["search_keys", "Search keywords", "wstr"],
];
const TYPES: GShopFieldType["type"][] = ["u8", "u16", "u32", "i32", "f32", "bool", "wstr", "str", "bytes", "group"];
const TYPE_NAMES: Record<string, string> = { u8: "u8 (1 byte)", u16: "u16 (2)", u32: "u32 (4)", i32: "i32 (4)", f32: "f32 (4)", bool: "bool (1)", wstr: "UTF-16 text", str: "GBK text", bytes: "bytes (unknown)", group: "group (repeated)" };

function sizeOf(field: GShopField): number {
  switch (field.type) {
    case "u8": case "bool": return 1;
    case "u16": return 2;
    case "u32": case "i32": case "f32": return 4;
    case "wstr": return (field.len ?? 0) * 2;
    case "str": case "bytes": return field.len ?? 0;
    case "group": return (field.count ?? 0) * (field.fields ?? []).reduce((total, child) => total + sizeOf(child), 0);
  }
}

const fits = (field: GShopField, kind: string) => kind === "int" ? ["u8", "u16", "u32", "i32"].includes(field.type) : kind === "flag" ? ["u8", "u16", "u32", "i32", "bool"].includes(field.type) : field.type === kind;

/** A field with a new type: lengths and children as the type needs, the meaning dropped when it no longer fits. */
function retype(field: GShopField, type: GShopFieldType["type"]): GShopField {
  const next: GShopField = { name: field.name, type, note: field.note, meaning: field.meaning };
  if (type === "wstr" || type === "str" || type === "bytes") next.len = field.len ?? (type === "wstr" ? 32 : 4);
  if (type === "group") { next.count = field.count ?? 2; next.fields = field.fields ?? [{ name: "value", type: "u32" }]; delete next.meaning; }
  const kind = MEANINGS.find(([key]) => key === next.meaning)?.[2];
  if (kind && !fits(next, kind)) delete next.meaning;
  return next;
}

function FieldRows({ fields, onChange, offset, depth, usedMeanings }: { fields: GShopField[]; onChange: (fields: GShopField[]) => void; offset: number; depth: number; usedMeanings: Set<string> }) {
  const update = (index: number, field: GShopField) => onChange(fields.map((entry, position) => (position === index ? field : entry)));
  const move = (index: number, step: number) => { const next = [...fields]; const [entry] = next.splice(index, 1); next.splice(index + step, 0, entry); onChange(next); };
  let at = offset;
  return <>{fields.map((field, index) => {
    const start = at;
    at += sizeOf(field);
    const name = field.name;
    return <Fragment key={index}>
      <tr className={depth ? "gshop-layout-child" : undefined}>
        <td className="mono muted">{depth ? "" : start}</td>
        <td><input className="dyn-input" style={{ marginLeft: depth * 18 }} value={name} onChange={(event) => update(index, { ...field, name: event.target.value.replace(/[^A-Za-z0-9_]/g, "_") })} /></td>
        <td><select className="task-form-select" value={field.type} onChange={(event) => update(index, retype(field, event.target.value as GShopFieldType["type"]))}>{TYPES.filter((type) => !(depth && type === "group")).map((type) => <option key={type} value={type}>{TYPE_NAMES[type]}</option>)}</select></td>
        <td>{(field.type === "wstr" || field.type === "str" || field.type === "bytes") && <input className="dyn-input gshop-layout-len" type="number" min={1} value={field.len ?? 1} title={field.type === "wstr" ? "Characters (2 bytes each)" : "Bytes"} onChange={(event) => update(index, { ...field, len: Math.max(1, Number(event.target.value) || 1) })} />}
          {field.type === "group" && <span className="npcgen-inline">× <input className="dyn-input gshop-layout-len" type="number" min={1} value={field.count ?? 1} onChange={(event) => update(index, { ...field, count: Math.max(1, Number(event.target.value) || 1) })} /></span>}</td>
        <td className="mono muted">{sizeOf(field)}</td>
        <td>{!depth && field.type !== "group" && <select className="task-form-select" value={field.meaning ?? ""} onChange={(event) => update(index, { ...field, meaning: event.target.value || undefined })}>
          <option value="">—</option>
          {MEANINGS.filter(([key, , kind]) => fits(field, kind) && (key === field.meaning || !usedMeanings.has(key))).map(([key, label]) => <option key={key} value={key}>{label}</option>)}
        </select>}</td>
        <td><input className="dyn-input wide" value={field.note ?? ""} placeholder="Note" onChange={(event) => update(index, { ...field, note: event.target.value })} /></td>
        <td><span className="task-form-row-actions">
          <button className="icon-btn small" disabled={index === 0} title="Move up" onClick={() => move(index, -1)}><ArrowUp size={12} /></button>
          <button className="icon-btn small" disabled={index === fields.length - 1} title="Move down" onClick={() => move(index, 1)}><ArrowDown size={12} /></button>
          <button className="icon-btn small" title="Insert a field below" onClick={() => onChange([...fields.slice(0, index + 1), { name: `field_${start + sizeOf(field)}`, type: "u32" }, ...fields.slice(index + 1)])}><Plus size={12} /></button>
          <button className="icon-btn small danger" title="Remove" disabled={fields.length === 1} onClick={() => onChange(fields.filter((_, position) => position !== index))}><Trash2 size={12} /></button>
        </span></td>
      </tr>
      {field.type === "group" && <FieldRows fields={field.fields ?? []} onChange={(children) => update(index, { ...field, fields: children })} offset={start} depth={depth + 1} usedMeanings={usedMeanings} />}
    </Fragment>;
  })}</>;
}

interface Props {
  /** The layout to start from (a built-in one is saved under a new ID). */
  start: GShopLayout;
  /** The file to preview on (none: the open shop's file). */
  path: string | null;
  /** The file's record size when known (e.g. from a file no layout fits). */
  recordSize: number | null;
  onSaved: (layout: GShopLayout) => void;
  onDeleted: (id: string) => void;
  onClose: () => void;
}

export function GShopLayoutEditor({ start, path, recordSize, onSaved, onDeleted, onClose }: Props) {
  const [layout, setLayout] = useState<GShopLayout>(() => start.builtin ? { ...start, id: `${start.id}-custom`, name: `${start.name.replace(/\s*\(.*\)$/, "")} (custom)`, builtin: false } : start);
  const [preview, setPreview] = useState<GShopLayoutPreview | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const size = useMemo(() => layout.fields.reduce((total, field) => total + sizeOf(field), 0), [layout.fields]);
  const target = preview?.recordSize ?? recordSize;
  const usedMeanings = useMemo(() => new Set(layout.fields.flatMap((field) => (field.meaning ? [field.meaning] : []))), [layout.fields]);

  // The preview follows every change (shortly after typing stops).
  useEffect(() => {
    const timer = window.setTimeout(() => {
      previewGShopLayout(path, layout, 4).then((next) => { setPreview(next); setPreviewError(null); }).catch((problem) => setPreviewError(String(problem).replace(/^Error: /, "")));
    }, 250);
    return () => window.clearTimeout(timer);
  }, [layout, path]);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      await saveGShopLayout(layout);
      onSaved(layout);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setSaving(false);
    }
  };
  const remove = async () => {
    if (!window.confirm(`Delete the layout ${layout.name}? Files read with it then need another layout.`)) return;
    try {
      await deleteGShopLayout(layout.id);
      onDeleted(layout.id);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    }
  };
  const fieldNames = preview?.rows[0]?.map(([field]) => field) ?? [];
  const sizeOk = target === null || target === undefined || size === target;

  // Fills the window; only Cancel or the close button leave it, so a stray click keeps the edits.
  return <div className="modal-backdrop gshop-layout-backdrop">
    <div className="modal gshop-layout-editor" role="dialog" aria-label="Shop layout">
      <header className="modal-head"><h3>Item layout</h3><span className="muted small">How the bytes of one shop item are read</span><span className="spacer" /><button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button></header>
      <div className="gshop-layout-top">
        <label className="npcgen-flag">ID <input className="dyn-input" value={layout.id} onChange={(event) => setLayout({ ...layout, id: event.target.value.replace(/[^A-Za-z0-9_-]/g, "-") })} /></label>
        <label className="npcgen-flag">Name <input className="dyn-input wide" value={layout.name} onChange={(event) => setLayout({ ...layout, name: event.target.value })} /></label>
        <label className="npcgen-flag gshop-layout-description">Note <input className="dyn-input wide" value={layout.description} onChange={(event) => setLayout({ ...layout, description: event.target.value })} /></label>
        <span className={"path-data-badge" + (sizeOk ? "" : " gshop-layout-bad")} title="The layout must be exactly as long as one item of the file">
          <b>Size:</b> {count(size)} bytes{target ? <> of {count(target)}{size === target ? " ✓" : size < target ? ` (${count(target - size)} missing)` : ` (${count(size - target)} too many)`}</> : ""}
        </span>
      </div>
      {error && <div className="path-data-message error">{error}</div>}
      <div className="gshop-layout-body">
        <div className="gshop-layout-fields">
          <table className="dyn-table">
            <thead><tr><th title="Byte offset in the item">At</th><th>Field</th><th>Type</th><th>Length</th><th>Bytes</th><th title="What the editor shows it as">Meaning</th><th>Note</th><th /></tr></thead>
            <tbody><FieldRows fields={layout.fields} onChange={(fields) => setLayout({ ...layout, fields })} offset={0} depth={0} usedMeanings={usedMeanings} /></tbody>
          </table>
          <div className="dyn-table-foot"><button className="btn small" onClick={() => setLayout({ ...layout, fields: [...layout.fields, { name: `field_${size}`, type: "bytes", len: Math.max(1, (target ?? size + 4) - size) }] })}><Plus size={12} /> Add a field at the end{target && target > size ? ` (${target - size} bytes)` : ""}</button></div>
        </div>
        <div className="gshop-layout-preview">
          <b className="small">Preview: the first items of the file</b>
          {previewError ? <div className="path-data-message error">{previewError}</div> : !preview ? <div className="empty-note"><Loader2 size={14} className="spin" /> Reading…</div> : <>
            <span className="muted small">{count(preview.items)} items, {preview.recordSize ? `${count(preview.recordSize)} bytes each` : "no items"}; categories: {preview.categories.slice(0, 8).join(", ")}</span>
            {!sizeOk && <div className="path-data-message"><CircleAlert size={13} /> The layout is {size < (target ?? 0) ? "shorter" : "longer"} than an item: fields after the difference show the next item's bytes. Fix the sizes until names and prices look right.</div>}
            <div className="gshop-layout-preview-table"><table className="dyn-table">
              <thead><tr><th>Field</th>{preview.rows.map((_, index) => <th key={index}>Item {index + 1}</th>)}</tr></thead>
              <tbody>{fieldNames.map((field, row) => <tr key={field}>
                <td className="mono small nowrap">{field}</td>
                {preview.rows.map((values, index) => <td key={index} className="small gshop-layout-value" title={values[row]?.[2]}>{values[row]?.[2]}</td>)}
              </tr>)}</tbody>
            </table></div>
          </>}
        </div>
      </div>
      <footer className="modal-foot">
        <span className="muted small">Saved layouts are kept in the app's config folder; a file is read with the first layout whose size fits it (yours before the built-in ones).</span>
        <span className="spacer" />
        {!start.builtin && layout.id === start.id && <button className="btn danger" onClick={() => void remove()}><Trash2 size={14} /> Delete</button>}
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn primary" disabled={saving || !sizeOk} title={sizeOk ? undefined : "The size must match the file's items"} onClick={() => void save()}>{saving ? <Loader2 size={14} className="spin" /> : <Save size={14} />} Save and use</button>
      </footer>
    </div>
  </div>;
}
