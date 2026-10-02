import { Fragment } from "react";
import { Link2, ScanSearch } from "lucide-react";
import type { ListSummary, ReferencedBy } from "../elements/types";

interface Props {
  data: ReferencedBy | null;
  lists: ListSummary[];
  /** Item icon URL for a path ID (when the client's icons are available). */
  icon?: (pathId?: number | null) => string | undefined;
  /** Open a referring record (in a new tab for Ctrl+click / middle-click). */
  onOpen: (list: number, row: number, newTab: boolean) => void;
}

/** Records that point at the selected record, grouped by list. */
export function ReferencedByTable({ data, lists, icon, onOpen }: Props) {
  if (!data) return <div className="empty-note">Looking for references…</div>;
  if (!data.referrers.length) {
    return (
      <div className="empty-note refby-empty">
        Nothing in this file refers to ID {data.id}. References are found through fields whose refs name this list,
        and through ID fields (such as id_goods or id_to_make) of the same ID space.
      </div>
    );
  }

  const groups = new Map<number, typeof data.referrers>();
  for (const r of data.referrers) groups.set(r.list, [...(groups.get(r.list) ?? []), r]);
  const withIcons = !!icon && data.referrers.some((r) => r.icon);

  return (
    <div className="refby scroll">
      <div className={"refby-row refby-head" + (withIcons ? " with-icons" : "")}>
        {withIcons && <span />}
        <span>Record</span>
        <span>ID</span>
        <span>Field</span>
        <span>Match</span>
      </div>
      {[...groups.entries()].map(([list, rows]) => (
        <Fragment key={list}>
          <div className="refby-group">
            <span className="list-num">{list}</span>
            <span className="truncate">{lists[list]?.name ?? `List ${list}`}</span>
            <span className="muted small">{rows.length}</span>
          </div>
          {rows.map((r) => (
            <div
              key={`${r.list}:${r.row}:${r.field}`}
              className={"refby-row" + (withIcons ? " with-icons" : "")}
              onClick={(e) => onOpen(r.list, r.row, e.ctrlKey || e.metaKey)}
              onAuxClick={(e) => {
                if (e.button === 1) {
                  e.preventDefault();
                  onOpen(r.list, r.row, true);
                }
              }}
              onMouseDown={(e) => e.button === 1 && e.preventDefault()}
              title="Open this record (Ctrl+click or middle-click: in a new tab)"
            >
              {withIcons &&
                (r.icon ? <img className="row-icon" src={icon!(r.icon)} alt="" loading="lazy" draggable={false} /> : <span />)}
              <span className="truncate">{r.name || <span className="muted">#{r.row}</span>}</span>
              <span className="mono muted">{r.id}</span>
              <span className="mono truncate" title={r.field}>
                {r.field}
              </span>
              <span
                className={"match-badge " + (r.how === "declared" ? "ok" : "warn")}
                title={
                  r.how === "declared"
                    ? "The field's refs name this list"
                    : "The field's name says it holds an ID of this kind, and its value is this record's ID"
                }
              >
                {r.how === "declared" ? (
                  <>
                    <Link2 size={11} /> ref
                  </>
                ) : (
                  <>
                    <ScanSearch size={11} /> by ID
                  </>
                )}
              </span>
            </div>
          ))}
        </Fragment>
      ))}
      {data.truncated && <div className="empty-note">Showing the first {data.referrers.length} references.</div>}
    </div>
  );
}
