import { useEffect, useMemo, useState } from "react";
import { Copy, FolderOpen } from "lucide-react";
import { count } from "../elements/format";
import type { GShopCompareRow, GShopComparison } from "../elements/types";

const STATUS: Record<GShopCompareRow["status"], string> = { missing: "Only in the other shop", different: "Different", only_here: "Only in this shop" };

/** Readable names of the item fields (layout meanings). */
const FIELDS: Record<string, string> = {
  id: "item", num: "count", icon: "icon", price: "price", time: "duration", discount: "discount", bonus: "bonus", props: "flags and schemes", category: "category",
  local_id: "local ID", description: "description", name: "name", has_present: "gift", present_name: "gift", present_id: "gift", present_count: "gift", present_time: "gift",
  present_icon: "gift", present_bind: "gift", present_description: "gift", valid_type: "sale window", valid_start: "sale window", valid_end: "sale window", valid_param: "sale window",
  search_keys: "keywords",
};
const describe = (fields: string[]) => [...new Set(fields.map((field) => FIELDS[field] ?? field))].join(", ");

interface Props {
  comparison: GShopComparison | null;
  kind: string;
  busy: boolean;
  labels: Record<string, string>;
  price: (value: number) => string;
  onChoose: () => void;
  onCopy: (picks: number[]) => void;
  onShow: (index: number) => void;
}

export function GShopCompare({ comparison, kind, busy, labels, price, onChoose, onCopy, onShow }: Props) {
  const [status, setStatus] = useState<GShopCompareRow["status"]>("missing");
  const [picked, setPicked] = useState<Set<number>>(new Set());
  useEffect(() => setPicked(new Set()), [comparison]);
  const counts = useMemo(() => {
    const out: Record<string, number> = {};
    for (const row of comparison?.rows ?? []) out[row.status] = (out[row.status] ?? 0) + 1;
    return out;
  }, [comparison]);
  const rows = (comparison?.rows ?? []).filter((row) => row.status === status);
  const copyable = (row: GShopCompareRow) => row.status !== "only_here" && !row.blocked && row.there !== null;
  const selectable = rows.filter(copyable);
  const toggle = (row: GShopCompareRow) => setPicked((current) => {
    const next = new Set(current);
    if (next.has(row.there!)) next.delete(row.there!);
    else next.add(row.there!);
    return next;
  });
  const pickedRows = (comparison?.rows ?? []).filter((row) => row.there !== null && picked.has(row.there));
  const skipped = comparison ? [...comparison.onlyThere, ...comparison.onlyHere].map((meaning) => FIELDS[meaning] ?? meaning) : [];

  return <div className="npcgen-nearby">
    <div className="npcgen-nearby-bar">
      {comparison ? <span className="mono truncate" title={comparison.path}>{comparison.path}</span> : <span className="muted">Choose another gshop file (another client, a server, a backup) or a JSON export.</span>}
      {comparison && <span className="path-data-badge"><b>{comparison.json ? "JSON export" : "Layout:"}</b> {comparison.json ? "" : comparison.layout}</span>}
      {comparison && comparison.kind !== kind && <span className="path-data-badge warn" title="Both are shops, but different ones"><b>{comparison.kind}</b> (this is the {kind.toLowerCase()})</span>}
      {comparison && <span className="path-data-badge"><b>Same:</b> {count(comparison.same)} of {count(comparison.items)}</span>}
      <span className="spacer" />
      <button className="btn" onClick={onChoose} disabled={busy}><FolderOpen size={14} /> {comparison ? "Choose another…" : "Choose a shop file or JSON…"}</button>
    </div>
    {!comparison ? <div className="empty-note">Items are paired by what they sell (item and count; repeated ones in file order) and categories by name. You can copy the items this shop lacks and replace the ones that differ. Only the fields both files store are compared and copied; copied items keep their category, and subcategories this shop lacks are added.</div> : <>
      <div className="npcgen-nearby-bar">
        <span className="dyn-compare-tabs">{(Object.keys(STATUS) as GShopCompareRow["status"][]).map((entry) => <button key={entry} className={"btn small" + (status === entry ? " active" : "")} onClick={() => { setStatus(entry); setPicked(new Set()); }}>{STATUS[entry]} <span className="muted">{count(counts[entry] ?? 0)}</span></button>)}</span>
        <span className="spacer" />
        {skipped.length > 0 && <span className="muted small" title="Only one of the two layouts stores these">Not compared: {[...new Set(skipped)].join(", ")}</span>}
      </div>
      <div className="npcgen-nearby-table">
        {rows.length ? <table className="dyn-table">
          <thead><tr>
            <th>{selectable.length > 0 && <input type="checkbox" checked={selectable.every((row) => picked.has(row.there!))} title="Select every row shown" onChange={(event) => setPicked((current) => { const next = new Set(current); for (const row of selectable) { if (event.target.checked) next.add(row.there!); else next.delete(row.there!); } return next; })} />}</th>
            <th>Item</th><th>Name</th><th>Sells</th><th>Price</th><th>Category</th><th>{status === "different" ? "Differs in" : ""}</th><th />
          </tr></thead>
          <tbody>{rows.map((row) => <tr key={`${row.there}:${row.here}`} className={row.there !== null && picked.has(row.there) ? "selected" : undefined}>
            <td>{copyable(row) && <input type="checkbox" checked={picked.has(row.there!)} onChange={() => toggle(row)} />}</td>
            <td className="nowrap">{row.here !== null ? <span className="muted small">{row.here + 1} here</span> : <span className="muted small">{(row.there ?? 0) + 1} there</span>}</td>
            <td className="truncate">{row.name || <span className="muted">(no name)</span>}</td>
            <td className="truncate"><span className="mono">{row.id}</span>{row.num > 1 && <span className="muted"> ×{row.num}</span>} {labels[String(row.id)]?.split(" › ").pop() ?? ""}</td>
            <td className="mono">{price(row.price)}</td>
            <td className="truncate muted small">{row.category}</td>
            <td className="muted small">{row.blocked ?? describe(row.fields)}</td>
            <td>{row.here !== null && <button className="btn small" onClick={() => onShow(row.here!)}>Show</button>}</td>
          </tr>)}</tbody>
        </table> : <div className="empty-note">Nothing here.</div>}
      </div>
      <footer className="npcgen-nearby-foot">
        <span className="muted small">{status === "different" ? "Replacing copies the compared fields and keeps the item's position." : status === "missing" ? "Added items go after the last item of their subcategory." : "Items only this shop has are listed for reference."}</span>
        <span className="spacer" />
        <button className="btn primary" disabled={!pickedRows.length || busy} onClick={() => onCopy(pickedRows.map((row) => row.there!))}><Copy size={14} /> Copy {count(pickedRows.length)} item{pickedRows.length === 1 ? "" : "s"}</button>
      </footer>
    </>}
  </div>;
}
