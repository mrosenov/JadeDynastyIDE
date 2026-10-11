import { useEffect, useMemo, useState } from "react";
import { Search, X } from "lucide-react";
import { essenceRows, iconUrl, skillNames } from "../elements/api";
import { count } from "../elements/format";
import { numbersOf, shares, showValue, type LuaEntry, type LuaScript, type LuaTable, type LuaValue } from "../elements/luaData";
import type { RecordRow } from "../elements/types";

/** Names the pet scripts give by key: horoscopes, origins, races, elements (from petbedge_interface.lua). */
export interface PetDictionary {
  /** `Horoscope["riyao"]` → "Sun (日曜)", `Race["renzu"]` → "人族", … */
  labels: Map<string, string>;
  /** AvailablePets key → pet template ID. */
  pets: Map<string, number>;
}

// The nine horoscopes (petbedge_grades.lua's comment names the class each one suits).
const HOROSCOPES: Record<string, string> = { riyao: "Sun", yueyao: "Moon", jinyao: "Metal", muyao: "Wood", shuiyao: "Water", huoyao: "Fire", tuyao: "Earth", luohou: "Rahu", jidu: "Ketu" };
/** Races (ZLuaPetBedgeRaceDesc keys; the Chinese names are listed in the comment under it). */
const RACES: Record<string, string> = { renzu: "Human · 人族", yaozu: "Demon · 妖族", guizu: "Ghost · 鬼族", yuzu: "Winged · 羽族", meizu: "Charm · 魅族", tianjiang: "Celestial · 天将", xianling: "Immortal spirit · 仙灵", shanjing: "Mountain spirit · 山精", yeshou: "Beast · 野兽", yishou: "Exotic beast · 异兽", shuizu: "Aquatic · 水族", chonggu: "Insect · 虫蛊", huanling: "Phantom · 幻灵", siling: "Undead · 死灵", yuansu: "Elemental · 元素", teshu: "Special · 特殊" };
const ORIGINS: Record<string, string> = { task: "Quest", gshop: "Item mall", npc: "NPC", lottery: "Lottery", drop: "Drop", combine: "Combine", catch: "Catch" };
/** The 13 grades (petbedge_grades.lua: 气血/精力/小攻/大攻/防御/命中/躲闪/眩晕/虚弱/定身/魅惑/昏睡/冰冻). */
export const GRADES = ["HP", "MP", "Min attack", "Max attack", "Defense", "Hit", "Dodge", "Stun res.", "Weakness res.", "Root res.", "Charm res.", "Sleep res.", "Freeze res."];

const isTable = (value: LuaValue): value is LuaTable => typeof value === "object" && value !== null && "table" in value;

/** The pet list and name tables of petbedge_interface.lua. */
export function petDictionary(script: LuaScript | null): PetDictionary {
  const labels = new Map<string, string>();
  const pets = new Map<string, number>();
  for (const entry of script?.entries ?? []) {
    const keyed = /^(\w+)\["([^"]+)"\]$/.exec(entry.target);
    if (!keyed) continue;
    const [, table, key] = keyed;
    if (table === "AvailablePets" && isTable(entry.value)) {
      const tid = entry.value.fields.find((field) => field.key === "pet_tid")?.value;
      if (typeof tid === "number") pets.set(key, tid);
      continue;
    }
    if (typeof entry.value !== "number") continue;
    const english = table === "Horoscope" ? HOROSCOPES[key] : table === "ServerOrigins" ? ORIGINS[key] : table === "Race" ? RACES[key] : undefined;
    const chinese = (entry.comment ?? "").replace(/^\/+/, "").trim();
    labels.set(`${table}["${key}"]`, english ? `${english}${chinese && !english.includes(chinese) ? ` · ${chinese}` : ""}` : chinese || key);
  }
  // Without petbedge_interface.lua the fixed names still apply.
  for (const [key, name] of Object.entries(HOROSCOPES)) if (!labels.has(`Horoscope["${key}"]`)) labels.set(`Horoscope["${key}"]`, name);
  for (const [key, name] of Object.entries(ORIGINS)) if (!labels.has(`ServerOrigins["${key}"]`)) labels.set(`ServerOrigins["${key}"]`, name);
  return { labels, pets };
}

/** A condition with the script's keys replaced by names. */
export function prettyCondition(text: string, dictionary: PetDictionary, petName: (tid: number) => string | undefined): string {
  return text
    .replace(/^(if|elseif) /, "")
    .replace(/AvailablePets\["([^"]+)"\]\.pet_tid/g, (whole, key: string) => { const tid = dictionary.pets.get(key); return tid ? `${petName(tid) ?? key} (${tid})` : whole; })
    .replace(/(\w+)\["([^"]+)"\]/g, (whole) => dictionary.labels.get(whole) ?? whole)
    .replace(/\bhoroscope\s*==\s*/g, "horoscope: ")
    .replace(/\b(server)?origin\s*==\s*/g, "origin: ")
    .replace(/\bpet_tid\s*==\s*/g, "pet: ")
    .replace(/pet:QueryRebornCnt\(\)\s*==\s*(\d+)/g, (_, n: string) => n === "0" ? "not reborn" : `reborn ${n}×`)
    .replace(/ and /g, " and ").replace(/ or /g, " or ");
}

/** Elements names of pet templates and items (cached). */
function useNames(ids: number[]) {
  const [names, setNames] = useState<Map<number, RecordRow>>(new Map());
  const key = [...new Set(ids)].sort((a, b) => a - b).join(",");
  useEffect(() => {
    const wanted = key ? key.split(",").map(Number).filter((id) => id > 0) : [];
    if (!wanted.length) return;
    essenceRows(wanted).then((found) => setNames(new Map(found.map((row) => [row.id, row])))).catch(() => {});
  }, [key]);
  return names;
}

function Conditions({ list, dictionary, petName }: { list: string[]; dictionary: PetDictionary; petName: (tid: number) => string | undefined }) {
  const shown = list.filter((condition) => !condition.startsWith("for "));
  if (!shown.length) return <span className="muted small">always</span>;
  return <span className="lua-conditions">{shown.map((condition, index) => <span key={index} className="lua-condition">{condition === "else" ? "otherwise" : prettyCondition(condition, dictionary, petName)}</span>)}</span>;
}

// ---------------------------------------------------------------- petbedge_interface.lua

/** The pets the server accepts (`ZLuaAvailablePets`) and the scripts' name tables. */
export function PetListView({ script, iconGeneration }: { script: LuaScript; iconGeneration: number | null }) {
  const dictionary = useMemo(() => petDictionary(script), [script]);
  const pets = useMemo(() => script.entries.filter((entry) => /^AvailablePets\["[^"]+"\]$/.test(entry.target) && isTable(entry.value)), [script]);
  const names = useNames(pets.flatMap((entry) => { const table = entry.value as LuaTable; return ["pet_tid", "spirit_id"].map((key) => table.fields.find((field) => field.key === key)?.value).filter((value): value is number => typeof value === "number"); }));
  const [query, setQuery] = useState("");
  const tables = useMemo(() => {
    const out = new Map<string, LuaEntry[]>();
    for (const entry of script.entries) {
      if (!/^\w+\["[^"]+"\]$/.test(entry.target) || typeof entry.value !== "number" || !entry.fn) continue;
      out.set(entry.fn, [...(out.get(entry.fn) ?? []), entry]);
    }
    return [...out.entries()].filter(([, entries]) => entries.length >= 2);
  }, [script]);
  const field = (table: LuaTable, key: string) => table.fields.find((entry) => entry.key === key)?.value;
  const text = query.trim().toLowerCase();
  const shown = pets.filter((entry) => {
    if (!text) return true;
    const tid = field(entry.value as LuaTable, "pet_tid");
    return entry.target.toLowerCase().includes(text) || (entry.comment ?? "").toLowerCase().includes(text) || String(tid) === text || (typeof tid === "number" && (names.get(tid)?.name ?? "").toLowerCase().includes(text));
  });
  return <div className="dyn-task-form-scroll drops-form">
    <section className="dyn-section">
      <header><b>Pets</b><span className="muted small">ZLuaAvailablePets: {count(pets.length)} pet templates the scripts accept</span><span className="spacer" /><span className="dyn-task-search lua-inline-search"><Search size={13} /><input value={query} placeholder="Pet name, key or ID" onChange={(event) => setQuery(event.target.value)} />{query && <button className="icon-btn small" onClick={() => setQuery("")}><X size={12} /></button>}</span></header>
      <div className="dyn-section-body lua-wide">
        <table className="dyn-table lua-table">
          <thead><tr><th>Pet</th><th>Script key</th><th>Race</th><th>Crit</th><th>Crit damage</th><th>Spirit item</th><th>Combine</th><th>Age</th><th>Free</th><th>Figure</th><th>Start grade</th><th>Born special</th><th>Line</th></tr></thead>
          <tbody>{shown.map((entry) => {
            const table = entry.value as LuaTable;
            const tid = field(table, "pet_tid");
            const spirit = field(table, "spirit_id");
            const race = field(table, "race");
            const flag = (key: string) => { const value = field(table, key); return value === 1 ? "yes" : value === 0 ? "no" : value === undefined ? "" : showValue(value); };
            const row = typeof tid === "number" ? names.get(tid) : undefined;
            return <tr key={entry.target}>
              <td><span className="lua-item">{row?.icon !== undefined && iconGeneration !== null ? <img src={iconUrl(iconGeneration, row.icon)} alt="" /> : <span className="lua-item-blank" />}<span className="truncate">{row?.name ?? entry.comment ?? ""}</span><span className="mono muted small">{typeof tid === "number" ? tid : ""}</span></span>{row && entry.comment && <div className="muted small">{entry.comment}</div>}</td>
              <td className="mono small">{/"([^"]+)"/.exec(entry.target)?.[1]}</td>
              <td className="small">{race === undefined ? "" : typeof race === "object" && race !== null && "expr" in race ? dictionary.labels.get(race.expr) ?? <span className="ai-warn" title="The race table has no such key: the race is nil when the script runs">{race.expr} (not in the race table)</span> : showValue(race)}</td>
              <td className="mono">{showValue(field(table, "crit") ?? "")}</td>
              <td className="mono">{showValue(field(table, "critdmg") ?? "")}</td>
              <td className="small">{typeof spirit === "number" && spirit > 0 ? `${names.get(spirit)?.name ?? ""} ${spirit}` : <span className="muted">none</span>}</td>
              <td>{flag("combine_able")}</td><td>{flag("fixage_able")}</td><td>{flag("free_able")}</td><td>{flag("changefigure_able")}</td>
              <td className="mono">{showValue(field(table, "default_potential") ?? "")}</td>
              <td className="mono">{showValue(field(table, "born_special") ?? "")}</td>
              <td className="mono muted small">{entry.line}</td>
            </tr>;
          })}</tbody>
        </table>
      </div>
    </section>
    {tables.map(([fn, entries]) => <section key={fn} className="dyn-section">
      <header><b>{fn}</b><span className="muted small">{entries.length} names</span></header>
      <div className="dyn-section-body"><table className="dyn-table lua-table"><thead><tr><th>Key</th><th>Value</th><th>Note</th></tr></thead><tbody>
        {entries.map((entry) => <tr key={entry.line + entry.target}><td className="mono small">{entry.target}</td><td className="mono">{showValue(entry.value)}</td><td className="small">{dictionary.labels.get(entry.target) ?? entry.comment ?? ""}</td></tr>)}
      </tbody></table></div>
    </section>)}
  </div>;
}

// ---------------------------------------------------------------- petbedge_grades.lua

/** Grades by horoscope and origin (`ZLuaPetBedgeSetGrades`) and the horoscope chances. */
export function PetGradesView({ script, dictionary }: { script: LuaScript; dictionary: PetDictionary }) {
  const tids = [...dictionary.pets.values()];
  const names = useNames(tids);
  const petName = (tid: number) => names.get(tid)?.name;
  const grades = script.entries.filter((entry) => ["defaultgrades", "potentialgrades", "gradelimits"].includes(entry.target) && numbersOf(entry.value).length >= 13 && entry.conditions.length > 0);
  const chances = script.entries.filter((entry) => /^horoscopechance/i.test(entry.target) && numbersOf(entry.value).length === 9);
  const horoscopeNames = Object.values(HOROSCOPES);
  // Consecutive entries under the same branch (two "else" branches of different ifs read alike).
  const groups = new Map<number, LuaEntry[]>();
  let previous: LuaEntry | null = null;
  for (const entry of grades) {
    const same = previous && previous.conditions.join("|") === entry.conditions.join("|") && entry.line - previous.line <= 3;
    const key = same ? [...groups.keys()].pop()! : entry.line;
    groups.set(key, [...(groups.get(key) ?? []), entry]);
    previous = entry;
  }
  const LABELS: Record<string, string> = { defaultgrades: "Starting grade (max)", potentialgrades: "Growth grade (start)", gradelimits: "Growth limit" };
  return <div className="dyn-task-form-scroll drops-form">
    {chances.length > 0 && <section className="dyn-section">
      <header><b>Horoscope chances</b><span className="muted small">which horoscope a new pet gets (ZLuaPetBedgeHoroscopeInfo)</span></header>
      <div className="dyn-section-body lua-wide"><table className="dyn-table lua-table"><thead><tr><th>List</th><th>Applies</th>{horoscopeNames.map((name) => <th key={name}>{name}</th>)}</tr></thead><tbody>
        {chances.map((entry) => { const weights = numbersOf(entry.value); const share = shares(weights); return <tr key={entry.line}>
          <td className="mono small">{entry.target}</td>
          <td className="small">{entry.comment ?? (entry.target.toLowerCase().includes("npc") ? "pets bought from NPCs" : "other pets")}</td>
          {share.map((value, index) => <td key={index} className="mono">{(value * 100).toFixed(1)}%<div className="muted small">{weights[index]}</div></td>)}
        </tr>; })}
      </tbody></table></div>
    </section>}
    <section className="dyn-section">
      <header><b>Grades</b><span className="muted small">per horoscope, origin and pet ({groups.size} cases); later cases in the script refine earlier ones</span></header>
      <div className="dyn-section-body lua-wide"><table className="dyn-table lua-table lua-grades">
        <thead><tr><th>When</th><th>List</th>{GRADES.map((grade) => <th key={grade}>{grade}</th>)}<th>Line</th></tr></thead>
        <tbody>{[...groups.values()].map((entries) => entries.map((entry, index) => <tr key={entry.line} className={index === 0 ? "lua-group-start" : undefined}>
          {index === 0 && <td rowSpan={entries.length}><Conditions list={entry.conditions} dictionary={dictionary} petName={petName} /></td>}
          <td className="small">{LABELS[entry.target] ?? entry.target}</td>
          {numbersOf(entry.value).slice(0, 13).map((value, at) => <td key={at} className="mono">{Number.isFinite(value) ? value : "?"}</td>)}
          <td className="mono muted small">{entry.line}</td>
        </tr>))}</tbody>
      </table></div>
    </section>
  </div>;
}

// ---------------------------------------------------------------- petbedge_skills.lua

/** Skill pools by horoscope, rebirth and pet (`skillslist` with `skillschance`) and how many skills a pet gets. */
export function PetSkillsView({ script, dictionary }: { script: LuaScript; dictionary: PetDictionary }) {
  const names = useNames([...dictionary.pets.values()]);
  const petName = (tid: number) => names.get(tid)?.name;
  const [skills, setSkills] = useState<Record<string, string>>({});
  const pools = useMemo(() => {
    const out: { fn: string; conditions: string[]; list: LuaEntry; chance: LuaEntry | null }[] = [];
    for (const entry of script.entries) {
      if (!/list$/i.test(entry.target) || !numbersOf(entry.value).length) continue;
      const chance = script.entries.find((other) => other.fn === entry.fn && other.conditions.join("|") === entry.conditions.join("|") && other.target === entry.target.replace(/list$/i, "chance")) ?? null;
      out.push({ fn: entry.fn, conditions: entry.conditions, list: entry, chance });
    }
    return out;
  }, [script]);
  const counts = script.entries.filter((entry) => /choosenumchance/i.test(entry.target));
  const ids = useMemo(() => [...new Set(pools.filter((pool) => /skill/i.test(pool.list.target)).flatMap((pool) => numbersOf(pool.list.value)).filter((id) => id > 0))], [pools]);
  useEffect(() => { if (ids.length) skillNames(ids).then(setSkills).catch(() => {}); }, [ids]);
  return <div className="dyn-task-form-scroll drops-form">
    {counts.length > 0 && <section className="dyn-section">
      <header><b>Number of skills</b><span className="muted small">how many skills a new pet draws</span></header>
      <div className="dyn-section-body lua-wide"><table className="dyn-table lua-table"><thead><tr><th>When</th>{numbersOf(counts[0].value).map((_, index) => <th key={index}>{index + 1}</th>)}</tr></thead><tbody>
        {counts.map((entry) => <tr key={entry.line}><td><Conditions list={entry.conditions} dictionary={dictionary} petName={petName} /></td>{shares(numbersOf(entry.value)).map((value, index) => <td key={index} className="mono">{(value * 100).toFixed(1)}%</td>)}</tr>)}
      </tbody></table></div>
    </section>}
    {[...new Set(pools.map((pool) => pool.fn))].map((fn) => <section key={fn} className="dyn-section">
      <header><b>{fn}</b><span className="muted small">{script.functions.find((entry) => entry.name === fn)?.comment ?? ""}</span></header>
      <div className="dyn-section-body lua-wide">
        {pools.filter((pool) => pool.fn === fn).map((pool) => {
          const values = numbersOf(pool.list.value);
          const weights = pool.chance ? numbersOf(pool.chance.value) : values.map(() => 1);
          const share = shares(weights);
          const isSkill = /skill/i.test(pool.list.target);
          return <div key={pool.list.line} className="lua-pool">
            <div className="lua-pool-head"><Conditions list={pool.conditions} dictionary={dictionary} petName={petName} /><span className="mono muted small">{pool.list.target} · line {pool.list.line}</span></div>
            <div className="lua-pool-items">{values.map((value, index) => <span key={index} className="lua-chip" title={pool.chance ? `weight ${weights[index]}` : "no chance list: equal"}>
              {isSkill ? <>{skills[String(value)] ?? "skill"} <span className="mono muted">{value}</span></> : <span className="mono">{value}</span>} <b>{(share[index] * 100).toFixed(1)}%</b>
            </span>)}</div>
          </div>;
        })}
      </div>
    </section>)}
  </div>;
}
