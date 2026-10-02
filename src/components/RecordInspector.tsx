import { useEffect, useState } from "react";
import type { ListSummary, RecordDetail, RecordRow } from "../elements/types";
import { LAYOUT_LABEL, hex, layoutHelp } from "../elements/format";
import { FieldTree } from "./FieldTree";
import { type Path, allPaths, nodeAt, pathAt } from "../elements/fieldPaths";
import { HexView } from "./HexView";
import { ArrowLeft, ChevronsDownUp, ChevronsUpDown } from "lucide-react";
import { ReadingsCard } from "./ReadingsCard";
import { TextCard } from "./TextCard";
import { hasBreaks, hasColours, isTextNode } from "../elements/text";
import { SetPopover } from "./SetPopover";
import { ReferencedByTable } from "./ReferencedByTable";
import { referencedBy } from "../elements/api";
import type { FieldNode, ReferencedBy } from "../elements/types";
import { isUndefinedNode, undefinedSpan } from "../elements/readings";
import type { FieldSpec } from "../schema/model";

interface Props {
  list: ListSummary;
  row: RecordRow | null;
  detail: RecordDetail | null;
  canGoBack: boolean;
  onBack: () => void;
  onFollow: (list: number, row: number, newTab?: boolean) => void;
  /** Open the schema editor with a field defined at this offset. */
  onDefine: (list: number, offset: number, spec: FieldSpec) => void;
  /** Item icon URL for a path ID (when the client's icons are available). */
  icon?: (pathId?: number | null) => string | undefined;
  /** Open the enums & masks editor at a set. */
  onEditSet?: (key: string) => void;
  /** All lists of the file (for naming referring lists). */
  lists: ListSummary[];
}

export function RecordInspector({ list, row, detail, canGoBack, onBack, onFollow, onDefine, icon, onEditSet, lists }: Props) {
  const [expanded, setExpanded] = useState<Set<Path>>(new Set());
  const [selected, setSelected] = useState<Path | null>(null);
  const [hovered, setHovered] = useState<Path | null>(null);
  // Offset whose possible readings are shown (undefined bytes only). It stays
  // while browsing records of the list, to compare readings between records.
  const [readOffset, setReadOffset] = useState<number | null>(null);
  // The selected text's preview was closed (until another field is selected).
  const [textClosed, setTextClosed] = useState(false);
  const [setPopover, setSetPopover] = useState<{ node: FieldNode; anchor: DOMRect } | null>(null);
  useEffect(() => setSetPopover(null), [detail]);
  // Fields, or the records that refer to this one. The choice stays while browsing.
  const [view, setView] = useState<"fields" | "refs">("fields");
  const [refs, setRefs] = useState<ReferencedBy | null>(null);
  useEffect(() => {
    setRefs(null);
    if (!detail) return;
    let cancelled = false;
    referencedBy(detail.list, detail.index)
      .then((r) => !cancelled && setRefs(r))
      .catch(() => !cancelled && setRefs({ id: 0, referrers: [], truncated: false }));
    return () => {
      cancelled = true;
    };
  }, [detail]);

  // Keep the expansion state while browsing records of the same list.
  useEffect(() => {
    setExpanded(new Set());
    setReadOffset(null);
  }, [list.index]);
  useEffect(() => setHovered(null), [detail]);

  if (!row || !detail) {
    return (
      <section className="pane inspector">
        <div className="empty-note center">Select a record to inspect it.</div>
      </section>
    );
  }

  const toggle = (path: Path) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });

  const pickOffset = (offset: number) => {
    const chain = pathAt(detail.nodes, offset);
    if (!chain.length) return;
    setExpanded((prev) => new Set([...prev, ...chain.slice(0, -1)]));
    setSelected(chain[chain.length - 1]);
    setTextClosed(false);
    setReadOffset(undefinedSpan(detail.nodes, offset) ? offset : null);
  };

  const selectNode = (path: Path) => {
    setSelected(path);
    setTextClosed(false);
    const node = nodeAt(detail.nodes, path);
    setReadOffset(node && isUndefinedNode(node) ? node.off : null);
  };

  const span = readOffset !== null ? undefinedSpan(detail.nodes, readOffset) : null;

  const active = nodeAt(detail.nodes, hovered ?? selected ?? "") ?? null;
  const focused = selected ? nodeAt(detail.nodes, selected) : null;
  // Texts with line breaks, colours or more than a row shows get a preview.
  const textNode =
    focused && !textClosed && isTextNode(focused) && (hasBreaks(focused.value!) || hasColours(focused.value!) || focused.value!.length > 48)
      ? focused
      : null;
  const unknownBytes = detail.nodes.filter((n) => n.unknown).reduce((sum, n) => sum + n.size, 0);

  return (
    <section className="pane inspector">
      <div className="inspector-head">
        <div className="inspector-title">
          {canGoBack && (
            <button className="back" onClick={onBack} title="Back to the previous record (Alt+←)">
              <ArrowLeft size={15} />
            </button>
          )}
          {icon?.(detail.icon) && <img className="record-icon" src={icon(detail.icon)} alt="" draggable={false} />}
          <h2 className="truncate">{row.name || <span className="muted">Unnamed record</span>}</h2>
          <span className={`badge fit-${detail.layout}`} title={layoutHelp(detail.layout, detail.layoutId)}>
            {LAYOUT_LABEL[detail.layout]}
            {detail.layoutId && <span className="badge-sub"> · {detail.layoutId}</span>}
          </span>
        </div>
        <dl className="facts">
          <div>
            <dt>ID</dt>
            <dd className="mono">{row.id}</dd>
          </div>
          <div>
            <dt>Index</dt>
            <dd className="mono">{row.index}</dd>
          </div>
          <div>
            <dt>File offset</dt>
            <dd className="mono">{hex(detail.fileOffset, 8)}</dd>
          </div>
          <div>
            <dt>Size</dt>
            <dd className="mono">
              {detail.bytes.length} B
              {detail.layoutSize !== null && detail.layoutSize !== detail.bytes.length && (
                <span className="muted"> (layout {detail.layoutSize} B)</span>
              )}
            </dd>
          </div>
          {unknownBytes > 0 && detail.layout !== "none" && (
            <div>
              <dt>Unknown</dt>
              <dd className="mono">{unknownBytes} B</dd>
            </div>
          )}
        </dl>
        {(detail.layout === "partial" || detail.layout === "grown") && (
          <p className="note">{layoutHelp(detail.layout, detail.layoutId)}</p>
        )}
      </div>

      <div className="inspector-body">
        <div className="subhead">
          <div className="insp-tabs" role="tablist">
            <button role="tab" aria-selected={view === "fields"} className={view === "fields" ? "active" : ""} onClick={() => setView("fields")}>
              Fields
            </button>
            <button
              role="tab"
              aria-selected={view === "refs"}
              className={view === "refs" ? "active" : ""}
              onClick={() => setView("refs")}
              title="Records of this file that point at this record's ID"
            >
              Referenced by
              <span className={"count-chip" + (refs?.referrers.length ? " on" : "")}>
                {refs ? refs.referrers.length + (refs.truncated ? "+" : "") : "…"}
              </span>
            </button>
          </div>
          <span className="spacer" />
          {view === "fields" && (
            <>
              <button className="link" onClick={() => setExpanded(new Set(allPaths(detail.nodes)))}>
                <ChevronsUpDown size={13} /> Expand all
              </button>
              <button className="link" onClick={() => setExpanded(new Set())}>
                <ChevronsDownUp size={13} /> Collapse all
              </button>
            </>
          )}
        </div>
        {view === "refs" ? (
          <ReferencedByTable data={refs} lists={lists} icon={icon} onOpen={onFollow} />
        ) : (
        <FieldTree
          nodes={detail.nodes}
          expanded={expanded}
          selected={selected}
          onToggle={toggle}
          onSelect={selectNode}
          onHover={setHovered}
          onFollow={onFollow}
          icon={icon}
          onSet={(node, anchor) => setSetPopover({ node, anchor })}
        />
        )}
        {setPopover && (
          <SetPopover
            node={setPopover.node}
            anchor={setPopover.anchor}
            onEdit={(key) => {
              setSetPopover(null);
              onEditSet?.(key);
            }}
            onClose={() => setSetPopover(null)}
          />
        )}
        <div className="readings-slot">
          {span && readOffset !== null ? (
            <ReadingsCard
              bytes={detail.bytes}
              offset={readOffset}
              span={span}
              actionLabel="Define in Schema Editor"
              onOffset={setReadOffset}
              onDefine={(offset, spec) => onDefine(list.index, offset, spec)}
            />
          ) : (
            textNode && <TextCard node={textNode} onClose={() => setTextClosed(true)} />
          )}
        </div>
        <div className="subhead">
          <span className="pane-title">Bytes</span>
          <span className="spacer" />
          {active && (
            <span className="muted mono">
              {active.name} · {hex(active.off)}–{hex(active.off + active.size - 1)} · {active.size} B
            </span>
          )}
        </div>
        <HexView
          bytes={detail.bytes}
          fileOffset={detail.fileOffset}
          highlight={active ? { off: active.off, size: active.size } : null}
          focusKey={focused ? `${detail.index}:${selected}` : null}
          onPick={pickOffset}
        />
      </div>
    </section>
  );
}
