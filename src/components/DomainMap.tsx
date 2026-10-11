import { useEffect, useMemo, useRef, useState } from "react";
import type { DomainEntry } from "../elements/types";

interface Props {
  domains: DomainEntry[];
  selected: number | null;
  onSelect: (index: number) => void;
  /** The selected domain's center was dragged to a new place. */
  onMoveCenter: (index: number, center: [number, number]) => void;
  /** Domains with problems (positions), outlined in the warning colour. */
  flagged: Set<number>;
  /** Names and classes the client uses instead of the file's (land.txt), by domain ID. */
  effective?: Map<number, { name: string; type: number }>;
}

const TYPE_COLORS = ["#8a979e", "#d9534f", "#e0a03a", "#3b82c4"];

/** The domains of a file as the client draws them: the triangles filled, the outline, the name at the center. */
export function DomainMap({ domains, selected, onSelect, onMoveCenter, flagged, effective }: Props) {
  const svg = useRef<SVGSVGElement>(null);
  const bounds = useMemo(() => {
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
    for (const domain of domains) {
      for (const [x, y] of [...domain.verts, domain.center]) {
        minX = Math.min(minX, x); minY = Math.min(minY, y); maxX = Math.max(maxX, x); maxY = Math.max(maxY, y);
      }
    }
    if (!Number.isFinite(minX)) return { x: 0, y: 0, w: 100, h: 100 };
    const pad = Math.max(maxX - minX, maxY - minY) * 0.04 + 10;
    return { x: minX - pad, y: minY - pad, w: maxX - minX + pad * 2, h: maxY - minY + pad * 2 };
  }, [domains]);
  const [view, setView] = useState(bounds);
  // A new file (other bounds) shows everything.
  useEffect(() => setView(bounds), [bounds]);
  const drag = useRef<{ kind: "pan" | "center"; startX: number; startY: number; view: typeof view; moved: boolean } | null>(null);
  const [dragCenter, setDragCenter] = useState<[number, number] | null>(null);
  /** The last press moved (a pan or a drag), so its click selects nothing. */
  const wasDrag = useRef(false);

  const toMap = (clientX: number, clientY: number): [number, number] => {
    const element = svg.current;
    const matrix = element?.getScreenCTM();
    if (!element || !matrix) return [0, 0];
    const point = element.createSVGPoint();
    point.x = clientX;
    point.y = clientY;
    const mapped = point.matrixTransform(matrix.inverse());
    return [mapped.x, mapped.y];
  };
  const onWheel = (event: React.WheelEvent) => {
    const [x, y] = toMap(event.clientX, event.clientY);
    const factor = event.deltaY > 0 ? 1.2 : 1 / 1.2;
    setView((current) => {
      const w = Math.min(bounds.w * 4, Math.max(20, current.w * factor));
      const h = current.h * (w / current.w);
      return { x: x - (x - current.x) * (w / current.w), y: y - (y - current.y) * (h / current.h), w, h };
    });
  };
  const onPointerDown = (event: React.PointerEvent) => {
    if (event.button !== 0) return;
    drag.current = { kind: "pan", startX: event.clientX, startY: event.clientY, view, moved: false };
    (event.target as Element).setPointerCapture?.(event.pointerId);
  };
  const onPointerMove = (event: React.PointerEvent) => {
    const state = drag.current;
    if (!state) return;
    if (Math.abs(event.clientX - state.startX) + Math.abs(event.clientY - state.startY) > 3) state.moved = true;
    if (state.kind === "center") {
      const [x, y] = toMap(event.clientX, event.clientY);
      setDragCenter([Math.round(x), Math.round(y)]);
      return;
    }
    const rect = svg.current?.getBoundingClientRect();
    if (!rect) return;
    const scale = Math.max(state.view.w / rect.width, state.view.h / rect.height);
    setView({ ...state.view, x: state.view.x - (event.clientX - state.startX) * scale, y: state.view.y - (event.clientY - state.startY) * scale });
  };
  const onPointerUp = () => {
    const state = drag.current;
    drag.current = null;
    wasDrag.current = !!state?.moved;
    if (state?.kind === "center" && dragCenter && selected !== null && state.moved) onMoveCenter(selected, dragCenter);
    setDragCenter(null);
  };

  const byId = useMemo(() => new Map(domains.map((domain) => [domain.id, domain])), [domains]);
  const chosen = selected !== null ? domains[selected] : null;
  const unit = Math.max(view.w, view.h) / 600;
  return <svg
    ref={svg}
    className="domain-map"
    viewBox={`${view.x} ${view.y} ${view.w} ${view.h}`}
    preserveAspectRatio="xMidYMid meet"
    onWheel={onWheel}
    onPointerDown={onPointerDown}
    onPointerMove={onPointerMove}
    onPointerUp={onPointerUp}
    onDoubleClick={() => setView(bounds)}
  >
    {domains.map((domain, index) => {
      const color = TYPE_COLORS[effective?.get(domain.id)?.type ?? domain.type] ?? TYPE_COLORS[0];
      const active = index === selected;
      const triangles = domain.faces
        .map((face) => face.map((at) => domain.verts[at]))
        .filter((corners) => corners.every(Boolean))
        .map((corners) => `M${corners[0][0]},${corners[0][1]} L${corners[1][0]},${corners[1][1]} L${corners[2][0]},${corners[2][1]} Z`)
        .join(" ");
      const outline = domain.verts.map(([x, y], at) => `${at ? "L" : "M"}${x},${y}`).join(" ") + " Z";
      return <g key={index} className={"domain-shape" + (active ? " active" : "")} onClick={(event) => { if (!wasDrag.current) { event.stopPropagation(); onSelect(index); } }}>
        <title>{`${domain.name || "(no name)"} · ID ${domain.id}`}</title>
        <path d={triangles} fill={color} fillOpacity={active ? 0.5 : 0.22} stroke="none" />
        <path d={outline} fill="none" stroke={flagged.has(index) ? "var(--warn)" : active ? "var(--accent)" : color} strokeWidth={active ? 2.5 : 1} vectorEffect="non-scaling-stroke" />
      </g>;
    })}
    {chosen && chosen.neighbours.map((id) => byId.get(id)).filter((other): other is DomainEntry => !!other).map((other, index) => {
      const from = dragCenter ?? chosen.center;
      return <line key={index} className="domain-link" x1={from[0]} y1={from[1]} x2={other.center[0]} y2={other.center[1]} vectorEffect="non-scaling-stroke" />;
    })}
    {domains.map((domain, index) => {
      const active = index === selected;
      const [x, y] = active && dragCenter ? dragCenter : domain.center;
      return <g key={`c${index}`} className={"domain-center" + (active ? " active" : "")}>
        <circle
          cx={x}
          cy={y}
          r={unit * (active ? 6 : 3.5)}
          onPointerDown={(event) => {
            if (!active || event.button !== 0) return;
            event.stopPropagation();
            drag.current = { kind: "center", startX: event.clientX, startY: event.clientY, view, moved: false };
            (event.target as Element).setPointerCapture?.(event.pointerId);
          }}
          onClick={(event) => { event.stopPropagation(); onSelect(index); }}
        >
          <title>{active ? "Drag to move the center (where the faction icon goes)" : domain.name}</title>
        </circle>
        <text x={x} y={y - unit * 8} fontSize={unit * 12} textAnchor="middle">{effective?.get(domain.id)?.name ?? (domain.name || domain.id)}</text>
      </g>;
    })}
  </svg>;
}
