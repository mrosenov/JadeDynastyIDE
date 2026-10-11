import type { MapBuffRegion, MapPrecinct, MapRegionFile, MapRegionItem, MapVec3 } from "./types";

/** `CELPrecinct::IsCrossLine` / `CELRegion::IsCrossLine`: does the ray from (x, z) towards +x cross edge `index`? */
function crossesLine(points: MapVec3[], x: number, z: number, index: number): boolean {
  const count = points.length;
  const v1 = points[index], v2 = points[(index + 1) % count];
  if (v1.x < x && v2.x < x) return false;
  if (v1.z < z && v2.z < z) return false;
  if (v1.z > z && v2.z > z) return false;
  if (v1.z === v2.z) return false;
  if (z === v1.z) {
    // The ray passes through vertex 1: look at the nearest earlier vertex off the ray.
    let previous: MapVec3 | null = null;
    for (let i = 1; i < count; i++) {
      const candidate = points[(index + count - i) % count];
      if (candidate.z !== z) { previous = candidate; break; }
    }
    if (!previous) return false;
    if ((previous.z < z && v2.z > z) || (previous.z > z && v2.z < z)) return false;
  }
  const slope = (v2.x - v1.x) / (v2.z - v1.z);
  const crossX = (z - v1.z) * slope + v1.x;
  return crossX > x;
}

/** `IsPointIn` of the client and server: bound box first, then the crossing count. */
export function pointIn(points: MapVec3[], x: number, z: number): boolean {
  if (!points.length) return false;
  let left = Infinity, right = -Infinity, top = Infinity, bottom = -Infinity;
  for (const point of points) { left = Math.min(left, point.x); right = Math.max(right, point.x); top = Math.min(top, point.z); bottom = Math.max(bottom, point.z); }
  if (x < left || x > right || z < top || z > bottom) return false;
  let crossings = 0;
  for (let i = 0; i < points.length; i++) if (crossesLine(points, x, z, i)) crossings++;
  return (crossings & 1) === 1;
}

const inBox = (pos: MapVec3, extents: MapVec3, spot: MapVec3) => Math.abs(spot.x - pos.x) <= extents.x && Math.abs(spot.y - pos.y) <= extents.y && Math.abs(spot.z - pos.z) <= extents.z;

export interface HereResult {
  /** Precincts of the instance containing the spot, in file order, and the one the game picks. */
  precincts: { index: number; precinct: MapPrecinct }[] | null;
  winner: { index: number; precinct: MapPrecinct } | null;
  /** Regions containing the spot (the client takes the first; the server treats any as a safe zone). */
  regions: { index: number; item: Extract<MapRegionItem, { kind: "region" }> }[] | null;
  /** The portal the client would report (the first of the instance whose box holds the spot, height included). */
  portal: { index: number; number: number; item: Extract<MapRegionItem, { kind: "portal" }> } | null;
  /** Portals of the instance whose box holds the spot on the ground plane but not at this height. */
  portalsAtOtherHeight: number;
  buffs: { index: number; buff: MapBuffRegion }[] | null;
}

/** What the game decides at a spot of an instance, from the precinct and region files given. */
export function whatsHere(files: MapRegionFile[], spot: MapVec3, instance: number): HereResult {
  const result: HereResult = { precincts: null, winner: null, regions: null, portal: null, portalsAtOtherHeight: 0, buffs: null };
  for (const file of files) {
    if (file.kind === "precinct") {
      result.precincts = [];
      file.precincts.forEach((precinct, index) => {
        if (precinct.instance !== instance || !pointIn(precinct.points, spot.x, spot.z)) return;
        result.precincts!.push({ index, precinct });
        // CELPrecinctSet::IsPointIn: the lowest priority number, the later one on a tie.
        if (!result.winner || precinct.priority <= result.winner.precinct.priority) result.winner = { index, precinct };
      });
    } else {
      result.regions = [];
      let number = 0;
      file.items.forEach((item, index) => {
        if (item.kind === "region" && pointIn(item.points, spot.x, spot.z)) result.regions!.push({ index, item });
        if (item.kind === "portal") {
          if (item.instance === instance && !result.portal) {
            if (inBox(item.pos, item.extents, spot)) result.portal = { index, number, item };
            else if (inBox(item.pos, { ...item.extents, y: Infinity }, spot)) result.portalsAtOtherHeight++;
          }
          number++;
        }
      });
      result.buffs = file.buffs.map((buff, index) => ({ index, buff })).filter(({ buff }) => pointIn(buff.points, spot.x, spot.z));
    }
  }
  return result;
}

/** Whether the level rule of a precinct applies (`gplayer_imp::TestRegion`), in words. */
export function levelRule(precinct: MapPrecinct): string | null {
  const { levelMin, levelMax, kickCity, rebirths } = precinct;
  if (levelMax < 0 || levelMin < 0 || kickCity <= 0 || rebirths < 0 || (levelMax === 0 && levelMin === 0)) return null;
  const base = rebirths * 150;
  const example = (level: number) => rebirths ? `level ${level} after ${rebirths} rebirth${rebirths === 1 ? "" : "s"}` : `level ${level}`;
  const range = [levelMin > 0 ? `at least ${levelMin + base} (${example(levelMin)})` : null, levelMax > 0 ? `at most ${levelMax + base} (${example(levelMax)})` : null].filter(Boolean).join(" and ");
  return `Level limit: level + 150 per rebirth must be ${range}; the server moves other players to the kick-out point (checked every 8 seconds).`;
}
