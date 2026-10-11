import { useMemo, useState } from "react";
import { Check, FolderOpen, Info, X } from "lucide-react";
import { count } from "../elements/format";
import type { QuizQuestion, QuizSource, QuizSourceQuestion } from "../elements/types";

type Status = "changed" | "there" | "here" | "same";
type Mode = "texts" | "whole";
type PairBy = "number" | "text";
const LABELS: Record<Status, string> = { changed: "Changed", there: "Only there", here: "Only here", same: "Same" };
const LETTERS = ["A", "B", "C", "D", "E", "F", "G", "H"];
const KINDS: Record<QuizSource["kind"], string> = { data: "question file", json: "JSON", csv: "CSV" };

interface Pair {
  /** Here: the question's number; only there: its number in the other file. */
  number: number;
  /** Position here (null: only there). */
  index: number | null;
  here: QuizQuestion | null;
  there: QuizSourceQuestion | null;
  status: Status;
  /** What differs, in words. */
  parts: string[];
}

interface Props {
  other: QuizSource;
  questions: QuizQuestion[];
  /** The questions after the import, as one undo step. */
  onApply: (next: QuizQuestion[], message: string) => void;
  onChooseOther: () => void;
  onClose: () => void;
}

const sameTexts = (here: QuizQuestion, there: QuizSourceQuestion) => here.text === there.text && here.answers.length === there.answers.length && here.answers.every((answer, index) => answer === there.answers[index]);

function differences(here: QuizQuestion, there: QuizSourceQuestion): string[] {
  const parts: string[] = [];
  if (here.text !== there.text) parts.push("question");
  if (here.answers.length !== there.answers.length) parts.push(`${here.answers.length} → ${there.answers.length} answers`);
  else if (here.answers.some((answer, index) => answer !== there.answers[index])) parts.push("answers");
  if (there.key !== null && there.key !== here.key) parts.push("correct answer");
  return parts;
}

/** Questions pair by number (position in the file: translations) or by question text (merging question banks; repeats in order). */
function pair(questions: QuizQuestion[], other: QuizSource, by: PairBy): Pair[] {
  const sorted = [...other.questions].sort((a, b) => a.number - b.number);
  const used = new Set<QuizSourceQuestion>();
  const queues = new Map<string, QuizSourceQuestion[]>();
  if (by === "text") for (const question of sorted) queues.set(question.text.trim(), [...(queues.get(question.text.trim()) ?? []), question]);
  const byNumber = new Map(sorted.map((question) => [question.number, question]));
  const out: Pair[] = questions.map((here, index) => {
    const there = (by === "text" ? queues.get(here.text.trim())?.shift() : byNumber.get(index + 1)) ?? null;
    if (!there) return { number: index + 1, index, here, there: null, status: "here", parts: [] };
    used.add(there);
    const parts = differences(here, there);
    return { number: index + 1, index, here, there, status: parts.length ? "changed" : "same", parts };
  });
  for (const there of sorted) if (!used.has(there)) out.push({ number: there.number, index: null, here: null, there, status: "there", parts: [] });
  return out;
}

/** Why a row cannot be taken in this mode (null: it can). */
function blocked(entry: Pair, mode: Mode): string | null {
  const { here, there } = entry;
  if (!there) return "not in the other file";
  if (mode === "texts") {
    if (!here) return "new questions come with Whole questions";
    if (there.answers.length !== here.answers.length) return "another number of answers: take the whole question";
    return sameTexts(here, there) ? "the same texts" : null;
  }
  if (entry.status === "same") return "the same";
  if (there.answers.length > 6) return "more than 6 answers";
  if (there.key === null && (!here || here.answers.length !== there.answers.length)) return "the file does not say which answer is correct";
  return null;
}

function Side({ question, other, keyOf }: { question: { text: string; answers: string[] } | null; other: { text: string; answers: string[] } | null; keyOf: number | null }) {
  if (!question) return <div className="ai-cmp-side empty muted small">—</div>;
  return <div className="ai-cmp-side">
    <div className={"quiz-cmp-text" + (other && other.text !== question.text ? " quiz-cmp-diff" : "")}>{question.text || <span className="muted">(no text)</span>}</div>
    <ol className="quiz-cmp-answers small">{question.answers.map((answer, index) => <li key={index} className={(other && other.answers[index] !== answer ? "quiz-cmp-diff" : "") + (keyOf !== null && keyOf & (1 << index) ? " quiz-cmp-correct" : "")}>
      <b>{LETTERS[index] ?? index + 1}</b> {answer || <span className="muted">(empty)</span>}{keyOf !== null && keyOf & (1 << index) ? <Check size={12} /> : null}
    </li>)}</ol>
    {keyOf === null && <div className="muted small">The file does not say which answer is correct.</div>}
  </div>;
}

/** The open questions against another question file, a JSON or a CSV (translations): pair by number, take texts or whole questions. */
export function QuizCompare({ other, questions, onApply, onChooseOther, onClose }: Props) {
  const [by, setBy] = useState<PairBy>(other.kind === "data" ? "text" : "number");
  const pairs = useMemo(() => pair(questions, other, by), [by, questions, other]);
  const counts = useMemo(() => {
    const out: Record<Status, number> = { changed: 0, there: 0, here: 0, same: 0 };
    for (const entry of pairs) out[entry.status]++;
    return out;
  }, [pairs]);
  const [mode, setMode] = useState<Mode>(other.kind === "data" ? "whole" : "texts");
  const [filter, setFilter] = useState<Status | "differ">("differ");
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [shown, setShown] = useState<number | null>(null);
  const rows = pairs.map((entry, index) => ({ entry, index, reason: blocked(entry, mode) })).filter(({ entry }) => filter === "differ" ? entry.status === "changed" || entry.status === "there" : entry.status === filter);
  const chosen = pairs.filter((entry, index) => picked.has(index) && !blocked(entry, mode));
  const selected = shown !== null ? pairs[shown] : rows[0]?.entry ?? null;
  const name = other.path.split(/[\\/]/).pop();
  const togglable = rows.filter(({ reason }) => !reason);

  const apply = () => {
    const next = [...questions];
    let replaced = 0;
    const added: QuizQuestion[] = [];
    for (const entry of chosen) {
      const there = entry.there!;
      if (entry.here) {
        next[entry.index!] = mode === "texts" ? { ...entry.here, text: there.text, answers: [...there.answers] } : { key: there.key ?? entry.here.key, text: there.text, answers: [...there.answers] };
        replaced++;
      } else added.push({ key: there.key!, text: there.text, answers: [...there.answers] });
    }
    next.push(...added);
    const what = [replaced ? `${mode === "texts" ? "texts of" : "replaced"} ${count(replaced)} question${replaced === 1 ? "" : "s"}` : "", added.length ? `added ${count(added.length)} at the end` : ""].filter(Boolean).join(", ");
    onApply(next, `From ${name}: ${what}. Undo (Ctrl+Z) takes it back.`);
    setPicked(new Set());
  };

  return <div className="modal-backdrop" onMouseDown={onClose}>
    <div className="modal ai-sim-dialog ai-compare-dialog" role="dialog" aria-label="Compare and import" onMouseDown={(event) => event.stopPropagation()}>
      <header className="modal-head"><h3>Compare with <span className="mono" title={other.path}>{name}</span></h3><span className="tag">{KINDS[other.kind]}</span><span className="muted small">{count(other.questions.length)} questions</span><span className="spacer" /><button className="btn small" onClick={onChooseOther}><FolderOpen size={13} /> Other file…</button><button className="icon-btn" onClick={onClose} aria-label="Close"><X size={16} /></button></header>
      {other.notes.map((note, index) => <div key={index} className="path-data-message"><Info size={14} /> {note}</div>)}
      <div className="npcgen-nearby-bar">
        <span className="dyn-compare-tabs">
          <button className={"btn small" + (filter === "differ" ? " active" : "")} onClick={() => setFilter("differ")}>Differ <span className="muted">{count(counts.changed + counts.there)}</span></button>
          {(Object.keys(LABELS) as Status[]).map((status) => <button key={status} className={"btn small" + (filter === status ? " active" : "")} onClick={() => setFilter(status)}>{LABELS[status]} <span className="muted">{count(counts[status])}</span></button>)}
        </span>
        <span className="spacer" />
        <span className="dyn-compare-tabs" title="How questions find their partner: by number for translations, by question text for other question banks">
          <span className="muted small">Pair by</span>
          <button className={"btn small" + (by === "number" ? " active" : "")} onClick={() => { setBy("number"); setPicked(new Set()); setShown(null); }}>Number</button>
          <button className={"btn small" + (by === "text" ? " active" : "")} onClick={() => { setBy("text"); setPicked(new Set()); setShown(null); }}>Question text</button>
        </span>
        <span className="dyn-compare-tabs" title="What a picked question takes from the other file">
          <button className={"btn small" + (mode === "texts" ? " active" : "")} onClick={() => setMode("texts")}>Texts only</button>
          <button className={"btn small" + (mode === "whole" ? " active" : "")} onClick={() => setMode("whole")}>Whole questions</button>
        </span>
      </div>
      <div className="ai-cmp-body">
        <div className="ai-cmp-list">
          <div className="ai-cmp-list-head">
            <input type="checkbox" title="Pick every question shown that can be taken" checked={togglable.length > 0 && togglable.every(({ index }) => picked.has(index))} onChange={(event) => setPicked((current) => { const next = new Set(current); for (const { index } of togglable) { if (event.target.checked) next.add(index); else next.delete(index); } return next; })} />
            <span className="muted small">{count(rows.length)} questions</span>
          </div>
          {rows.map(({ entry, index, reason }) => <div key={index} className={"ai-cmp-row quiz-cmp-row" + (selected === entry ? " selected" : "")} onClick={() => setShown(index)}>
            {!reason ? <input type="checkbox" checked={picked.has(index)} onClick={(event) => event.stopPropagation()} onChange={() => setPicked((current) => { const next = new Set(current); if (next.has(index)) next.delete(index); else next.add(index); return next; })} /> : <span />}
            <span className="mono muted">{entry.number}</span>
            <span className="truncate">{(entry.here ?? entry.there)?.text || <span className="muted">(no text)</span>}</span>
            <span className={"tag ai-cmp-" + entry.status}>{LABELS[entry.status].toLowerCase()}</span>
            {(entry.parts.length > 0 || (reason && entry.status !== "same" && entry.status !== "here")) && <span className="muted small truncate quiz-cmp-parts" title={reason ?? undefined}>{entry.parts.join(", ")}{reason && entry.status !== "same" && entry.status !== "here" && <span className="ai-warn">{entry.parts.length ? " · " : ""}{reason}</span>}</span>}
          </div>)}
          {!rows.length && <div className="empty-note">No question.</div>}
        </div>
        <div className="ai-cmp-detail">
          {!selected ? <div className="empty-note">Select a question.</div> : <>
            <div className="ai-cmp-detail-head"><b>{selected.index === null ? `Question ${selected.number} there` : `Question ${selected.number}`}</b> <span className={"tag ai-cmp-" + selected.status}>{LABELS[selected.status].toLowerCase()}</span>{selected.parts.length > 0 && <span className="muted small">{selected.parts.join(", ")}</span>}</div>
            {(() => { const reason = blocked(selected, mode); return reason && selected.status !== "same" && selected.status !== "here" ? <p className="ai-warn small">Cannot be taken now: {reason}.</p> : null; })()}
            <div className="ai-cmp-pairs">
              <div className="ai-cmp-pair-head drops-cmp-head"><span>Here</span><span>{name}</span></div>
              <div className="ai-cmp-pair drops-cmp-pair">
                <Side question={selected.here} other={selected.there} keyOf={selected.here?.key ?? null} />
                <Side question={selected.there} other={selected.here} keyOf={selected.there ? selected.there.key : null} />
              </div>
            </div>
          </>}
        </div>
      </div>
      <footer className="npcgen-nearby-foot">
        <span className="muted small">{mode === "texts" ? "Texts only: the question and answer texts, for translations; the correct answers stay. Questions with another number of answers need Whole questions." : "Whole questions: texts and correct answers (kept when the file does not say); questions only there are added at the end."} One undo step.</span>
        <span className="spacer" />
        <button className="btn" onClick={onClose}>Close</button>
        <button className="btn primary" disabled={!chosen.length} onClick={apply}><Check size={14} /> Take {count(chosen.length)} question{chosen.length === 1 ? "" : "s"}</button>
      </footer>
    </div>
  </div>;
}
