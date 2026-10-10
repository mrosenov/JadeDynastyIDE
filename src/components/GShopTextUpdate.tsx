import { useEffect, useMemo, useState } from "react";
import { Check, Loader2, X } from "lucide-react";
import { applyGShopTexts, gshopTextUpdates } from "../elements/api";
import { count } from "../elements/format";
import type { GShopTextField, GShopTextPreview, GShopTextUpdate, GShopView } from "../elements/types";

const TITLES: Record<GShopTextField, { title: string; source: string; one: string }> = {
  name: { title: "Update names from elements.data", source: "the open elements.data", one: "name" },
  description: { title: "Update descriptions from item_ext_desc.txt", source: "the client's configs.pck (item_ext_desc.txt)", one: "description" },
};

/** Line breaks as the shop stores them (a literal \r) shown as real ones. */
const shown = (text: string) => text.replace(/\\r|\r\n|\r|\n/g, "\n");
/** True when two texts differ only in spaces (around line breaks or at the ends). */
const spacesOnly = (a: string, b: string) => {
  const squeeze = (text: string) => shown(text).split("\n").map((line) => line.trim()).join("\n").trim();
  return squeeze(a) === squeeze(b);
};

interface Props {
  field: GShopTextField;
  view: GShopView;
  /** Positions of the items shown in the list. */
  shownItems: number[];
  onApplied: (view: GShopView, changed: number) => void;
  onClose: () => void;
}

export function GShopTextUpdate({ field, view, shownItems, onApplied, onClose }: Props) {
  const [scope, setScope] = useState<"all" | "shown">("all");
  const [preview, setPreview] = useState<GShopTextPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [withSpaces, setWithSpaces] = useState(false);
  const titles = TITLES[field];
  const filtered = shownItems.length !== view.items.length;

  useEffect(() => {
    let cancelled = false;
    setPreview(null);
    setError(null);
    gshopTextUpdates(field, scope === "all" ? null : shownItems)
      .then((next) => { if (!cancelled) setPreview(next); })
      .catch((problem) => { if (!cancelled) setError(String(problem).replace(/^Error: /, "")); });
    return () => { cancelled = true; };
    // The shown items are read when the scope changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [field, scope]);

  const rows = useMemo(() => (preview?.rows ?? []).filter((row) => withSpaces || !spacesOnly(row.current, row.text)), [preview, withSpaces]);
  const spaceRows = useMemo(() => (preview?.rows ?? []).filter((row) => spacesOnly(row.current, row.text)).length, [preview]);
  const usable = (row: GShopTextUpdate) => !row.problem;
  // Everything usable starts ticked.
  useEffect(() => setPicked(new Set(rows.filter(usable).map((row) => row.index))), [rows]);
  const chosen = rows.filter((row) => usable(row) && picked.has(row.index));
  const toggle = (index: number) => setPicked((current) => {
    const next = new Set(current);
    if (next.has(index)) next.delete(index);
    else next.add(index);
    return next;
  });

  const apply = async () => {
    setBusy(true);
    setError(null);
    try {
      const next = await applyGShopTexts(field, chosen.map((row) => ({ index: row.index, current: row.current, text: row.text })));
      onApplied(next, chosen.length);
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  };

  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal npcgen-nearby-dialog gshop-text-update" role="dialog" aria-label={titles.title} onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head"><h3>{titles.title}</h3><span className="muted small">Items whose {titles.one} differs from {titles.source}</span><span className="spacer" /><button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button></header>
      <div className="npcgen-nearby-bar">
        <span className="dyn-compare-tabs">
          <button className={"btn small" + (scope === "all" ? " active" : "")} onClick={() => setScope("all")}>All items <span className="muted">{count(view.items.length)}</span></button>
          <button className={"btn small" + (scope === "shown" ? " active" : "")} disabled={!filtered || !shownItems.length} title={filtered ? undefined : "The list shows every item"} onClick={() => setScope("shown")}>Shown in the list <span className="muted">{count(shownItems.length)}</span></button>
        </span>
        {preview && <span className="muted small">{count(preview.checked)} checked: {count(preview.rows.length)} differ, {count(preview.same)} already equal, {count(preview.missing)} without a {field === "name" ? "name in elements.data" : "description in the client"}</span>}
        <span className="spacer" />
        {spaceRows > 0 && <label className="npcgen-flag" title="Texts that differ only in spaces at line ends"><input type="checkbox" checked={withSpaces} onChange={(event) => setWithSpaces(event.target.checked)} /> Include {count(spaceRows)} that differ only in spaces</label>}
      </div>
      {error && <div className="path-data-message error">{error}</div>}
      <div className="npcgen-nearby-table">
        {!preview ? !error && <div className="empty-note"><Loader2 size={14} className="spin" /> Reading {titles.source}…</div>
          : !rows.length ? <div className="empty-note">Nothing to update.</div>
          : <table className="dyn-table gshop-text-table">
            <thead><tr>
              <th><input type="checkbox" checked={rows.filter(usable).every((row) => picked.has(row.index))} title="Select every row" onChange={(event) => setPicked(event.target.checked ? new Set(rows.filter(usable).map((row) => row.index)) : new Set())} /></th>
              <th>Item</th><th>Now</th><th>New</th>
            </tr></thead>
            <tbody>{rows.map((row) => <tr key={row.index} className={picked.has(row.index) && usable(row) ? "selected" : undefined}>
              <td>{usable(row) ? <input type="checkbox" checked={picked.has(row.index)} onChange={() => toggle(row.index)} /> : null}</td>
              <td className="nowrap"><span className="muted small">{row.index + 1}</span> <span className="mono">{row.id}</span>{field === "description" && <div className="small truncate gshop-text-name">{view.items[row.index]?.name}</div>}</td>
              <td className="gshop-text-cell muted">{shown(row.current) || <i>(empty)</i>}</td>
              <td className="gshop-text-cell">{shown(row.text)}{row.problem && <div className="path-data-message error small">{row.problem}: shorten it in the client or edit the item by hand</div>}</td>
            </tr>)}</tbody>
          </table>}
      </div>
      <footer className="npcgen-nearby-foot">
        <span className="muted small">{field === "name" ? "Names are read from the elements.data open in the Elements workspace." : "Line breaks are stored as the shop expects; colour codes are kept."} One undo step.</span>
        <span className="spacer" />
        <button className="btn" onClick={onClose}>Cancel</button>
        <button className="btn primary" disabled={!chosen.length || busy} onClick={() => void apply()}>{busy ? <Loader2 size={14} className="spin" /> : <Check size={14} />} Update {count(chosen.length)} item{chosen.length === 1 ? "" : "s"}</button>
      </footer>
    </div>
  </div>;
}
