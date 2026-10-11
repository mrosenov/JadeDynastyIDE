import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, ArrowDown, ArrowUp, CalendarDays, CircleAlert, Copy, FolderOpen, Info, ListChecks, Loader2, MessageCircleQuestion, Plus, Redo2, Save, Search, Trash2, TriangleAlert, Undo2, X } from "lucide-react";
import { openQuiz, quizProblems, saveQuiz } from "../elements/api";
import { bytes, count } from "../elements/format";
import type { QuizProblem, QuizQuestion } from "../elements/types";
import { TextInput } from "./DynTasksEditor";

export interface QuizEditorState {
  loaded: boolean;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  path: string | null;
  questions: number;
}

export interface QuizEditorHandle {
  choose: () => void;
  openPath: (path: string) => void;
  save: () => void;
  saveAs: () => void;
  undo: () => void;
  redo: () => void;
  cloneSelected: () => void;
  deleteSelected: () => void;
  openProblems: () => void;
  showCalendar: () => void;
}

interface Props {
  active: boolean;
  onStateChange: (state: QuizEditorState) => void;
}

interface History {
  states: QuizQuestion[][];
  cursor: number;
  keys: string[];
}

const SET = 30;
const LETTERS = ["A", "B", "C", "D", "E", "F"];
const LAST_KEY = "jdide.quiz.lastPath";
const BACKUP_KEY = "jdide.quiz.backup";
const messageOf = (problem: unknown) => String(problem).replace(/^Error: /, "");
/** The set a contest uses on a day of the month, as the server picks it. */
const setOfDay = (day: number, sets: number) => sets ? (day >= sets ? day % sets : day) : null;
const correctLetters = (question: QuizQuestion) => LETTERS.filter((_, index) => question.key & (1 << index)).join("");

/** zhuxian_question.data: the questions of gdeliveryd's daily quiz contest. */
export const QuizEditor = forwardRef<QuizEditorHandle, Props>(function QuizEditor({ active, onStateChange }, ref) {
  const [meta, setMeta] = useState<{ path: string; size: number; token: string } | null>(null);
  const [history, setHistory] = useState<History>({ states: [], cursor: -1, keys: [] });
  const [selected, setSelected] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [view, setView] = useState<"questions" | "calendar">("questions");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [backup, setBackup] = useState(() => { try { return localStorage.getItem(BACKUP_KEY) !== "0"; } catch { return true; } });
  const [problems, setProblems] = useState<QuizProblem[] | null>(null);
  const [problemsOpen, setProblemsOpen] = useState(false);
  const saved = useRef<QuizQuestion[] | null>(null);
  const autoOpened = useRef(false);
  const backedUp = useRef(new Set<string>());
  const rowsRef = useRef<HTMLDivElement>(null);

  const questions = history.cursor >= 0 ? history.states[history.cursor] : null;
  const dirty = !!questions && questions !== saved.current;

  const load = useCallback(async (path: string) => {
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const opened = await openQuiz(path);
      saved.current = opened.questions;
      setMeta({ path: opened.path, size: opened.size, token: opened.token });
      setHistory({ states: [opened.questions], cursor: 0, keys: [] });
      setSelected(opened.questions.length ? 0 : null);
      setQuery("");
      try { localStorage.setItem(LAST_KEY, opened.path); } catch { /* optional */ }
    } catch (problem) {
      setError(messageOf(problem));
    } finally {
      setBusy(false);
    }
  }, []);
  useEffect(() => {
    if (autoOpened.current) return;
    autoOpened.current = true;
    let last: string | null = null;
    try { last = localStorage.getItem(LAST_KEY); } catch { /* optional */ }
    if (last) void load(last);
  }, [load]);
  const guard = useCallback((action: () => void) => {
    if (!dirty || window.confirm("Discard the unsaved quiz question changes?")) action();
  }, [dirty]);
  const choose = useCallback(async () => {
    const picked = await open({ multiple: false, directory: false, defaultPath: meta?.path, title: "Open zhuxian_question.data (gdeliveryd contest_file)", filters: [{ name: "zhuxian_question.data", extensions: ["data"] }, { name: "All files", extensions: ["*"] }] });
    if (typeof picked === "string") guard(() => void load(picked));
  }, [guard, load, meta?.path]);

  useEffect(() => {
    if (!questions) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      quizProblems(questions).then((found) => { if (!cancelled) setProblems(found); }).catch((problem) => { if (!cancelled) setError(messageOf(problem)); });
    }, 350);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [questions]);

  const commit = useCallback((next: QuizQuestion[], key: string) => {
    setHistory((current) => {
      if (current.cursor < 0) return current;
      const states = current.states.slice(0, current.cursor + 1);
      const keys = current.keys.slice(0, current.cursor);
      if (current.cursor > 0 && keys[current.cursor - 1] === key) { states[current.cursor] = next; return { states, cursor: current.cursor, keys }; }
      states.push(next);
      keys.push(key);
      if (states.length > 301) { states.shift(); keys.shift(); return { states, cursor: states.length - 1, keys }; }
      return { states, cursor: current.cursor + 1, keys };
    });
    setNote(null);
  }, []);
  const setQuestion = (index: number, change: Partial<QuizQuestion>, key: string) => {
    if (!questions) return;
    commit(questions.map((question, at) => at === index ? { ...question, ...change } : question), `${index}:${key}`);
  };
  const undo = useCallback(() => setHistory((current) => current.cursor > 0 ? { ...current, cursor: current.cursor - 1 } : current), []);
  const redo = useCallback(() => setHistory((current) => current.cursor + 1 < current.states.length ? { ...current, cursor: current.cursor + 1 } : current), []);

  /** Moves a question to another position (later questions shift, so sets can change). */
  const moveTo = useCallback((from: number, to: number) => {
    if (!questions || to < 0 || to >= questions.length || from === to) return;
    const next = [...questions];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    commit(next, `move:${Date.now()}`);
    setSelected(to);
  }, [commit, questions]);
  const cloneSelected = useCallback(() => {
    if (!questions || selected === null) return;
    const next = [...questions];
    next.splice(selected + 1, 0, { ...questions[selected], answers: [...questions[selected].answers] });
    commit(next, `clone:${Date.now()}`);
    setSelected(selected + 1);
    setNote("Cloned below the original. Later questions move one place on, so the last question of each later set moves into the next set.");
  }, [commit, questions, selected]);
  const deleteSelected = useCallback(() => {
    if (!questions || selected === null) return;
    const question = questions[selected];
    if (!window.confirm(`Delete question ${selected + 1} (“${question.text.slice(0, 60)}”)?\n\nLater questions move one place back, so each later set takes the first question of the next set. Undo brings it back.`)) return;
    commit(questions.filter((_, index) => index !== selected), `delete:${Date.now()}`);
    setSelected(questions.length > 1 ? Math.min(selected, questions.length - 2) : null);
  }, [commit, questions, selected]);
  const addQuestion = () => {
    if (!questions) return;
    const at = selected === null ? questions.length : selected + 1;
    const next = [...questions];
    next.splice(at, 0, { key: 1, text: "", answers: ["", "", "", ""] });
    commit(next, `add:${Date.now()}`);
    setSelected(at);
  };

  const saveTo = useCallback(async (target: string, replaceChanged = false) => {
    if (!questions || !meta || busy) return;
    const errors = (problems ?? []).filter((problem) => problem.severity === "error");
    if (!replaceChanged && errors.length && !window.confirm(`gdeliveryd may not start or may crash with this file:\n\n${errors.slice(0, 6).map((problem) => `• ${problem.question !== null ? `Question ${problem.question + 1}: ` : ""}${problem.message}`).join("\n")}${errors.length > 6 ? "\n…" : ""}\n\nSave anyway?`)) { setProblemsOpen(true); return; }
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const same = target.toLowerCase() === meta.path.toLowerCase();
      const report = await saveQuiz(target, same ? meta.token : null, questions, backup && !backedUp.current.has(target.toLowerCase()), replaceChanged);
      backedUp.current.add(target.toLowerCase());
      saved.current = questions;
      setMeta({ path: report.path, size: report.size, token: report.token });
      try { localStorage.setItem(LAST_KEY, report.path); } catch { /* optional */ }
      setNote(`Saved ${count(questions.length)} questions to ${report.path}.${report.backup ? ` Backup: ${report.backup}.` : ""} gdeliveryd reads it when it starts (gamesys.conf contest_file): restart it.`);
    } catch (problem) {
      const message = messageOf(problem);
      if (message.startsWith("CHANGED_ON_DISK:") && window.confirm(`${message.slice("CHANGED_ON_DISK:".length).trim()}\n\nReplace it with the open questions?`)) {
        setBusy(false);
        await saveTo(target, true);
        return;
      }
      setError(message);
    } finally {
      setBusy(false);
    }
  }, [backup, busy, meta, problems, questions]);
  const saveCurrent = useCallback(() => { if (meta && dirty) void saveTo(meta.path); }, [dirty, meta, saveTo]);
  const saveAs = useCallback(async () => {
    if (!meta || busy) return;
    const target = await save({ defaultPath: meta.path, title: "Save zhuxian_question.data as", filters: [{ name: "zhuxian_question.data", extensions: ["data"] }] });
    if (typeof target === "string") await saveTo(target);
  }, [busy, meta, saveTo]);

  useImperativeHandle(ref, () => ({ choose: () => void choose(), openPath: (path) => guard(() => void load(path)), save: saveCurrent, saveAs: () => void saveAs(), undo, redo, cloneSelected, deleteSelected, openProblems: () => setProblemsOpen(true), showCalendar: () => setView("calendar") }), [choose, cloneSelected, deleteSelected, guard, load, redo, saveAs, saveCurrent, undo]);
  useEffect(() => onStateChange({ loaded: !!questions, dirty, canUndo: history.cursor > 0, canRedo: history.cursor + 1 < history.states.length, path: meta?.path ?? null, questions: questions?.length ?? 0 }), [dirty, history.cursor, history.states.length, meta?.path, onStateChange, questions]);

  useEffect(() => {
    if (!active) return;
    const onKey = (event: KeyboardEvent) => {
      const mod = event.ctrlKey || event.metaKey;
      const typing = event.target instanceof HTMLElement && /^(INPUT|TEXTAREA|SELECT)$/.test(event.target.tagName);
      const key = event.key.toLowerCase();
      if (event.key === "Escape" && problemsOpen) { event.preventDefault(); setProblemsOpen(false); return; }
      if (mod && event.shiftKey && key === "m") { event.preventDefault(); event.stopImmediatePropagation(); setProblemsOpen(true); return; }
      if (mod && ["o", "s", "z", "y", "d"].includes(key)) {
        if (typing && (key === "z" || key === "y")) return;
        event.preventDefault();
        event.stopImmediatePropagation();
        if (key === "o") void choose();
        else if (key === "s") event.shiftKey ? void saveAs() : saveCurrent();
        else if (key === "z") event.shiftKey ? redo() : undo();
        else if (key === "y") redo();
        else cloneSelected();
      } else if (event.key === "Delete" && !typing) {
        event.preventDefault();
        deleteSelected();
      } else if (event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown") && !typing && selected !== null) {
        event.preventDefault();
        moveTo(selected, selected + (event.key === "ArrowUp" ? -1 : 1));
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [active, choose, cloneSelected, deleteSelected, moveTo, problemsOpen, redo, saveAs, saveCurrent, selected, undo]);

  const flagged = useMemo(() => {
    const out = new Map<number, string>();
    for (const problem of problems ?? []) if (problem.question !== null && problem.severity !== "note" && (problem.severity === "error" || !out.has(problem.question))) out.set(problem.question, problem.severity);
    return out;
  }, [problems]);
  const sets = questions ? Math.floor(questions.length / SET) : 0;
  const daysOfSet = useMemo(() => {
    const out = new Map<number, number[]>();
    for (let day = 1; day <= 31; day++) { const set = setOfDay(day, sets); if (set !== null) out.set(set, [...(out.get(set) ?? []), day]); }
    return out;
  }, [sets]);
  const visible = useMemo(() => {
    const text = query.trim().toLowerCase();
    return (questions ?? []).map((question, index) => ({ question, index })).filter(({ question, index }) => !text || String(index + 1) === text || question.text.toLowerCase().includes(text) || question.answers.some((answer) => answer.toLowerCase().includes(text)));
  }, [query, questions]);
  useEffect(() => { if (selected !== null) document.querySelector(".quiz-row.selected")?.scrollIntoView({ block: "nearest" }); }, [selected, view]);

  if (!questions || !meta) return <section className="path-data-pane empty">
    <div className="drop-card path-data-empty">
      <MessageCircleQuestion size={30} />
      <h2>Open zhuxian_question.data</h2>
      <p className="muted">The questions of the daily quiz contest (19:00). gdeliveryd reads them (gamesys.conf <span className="mono">contest_file</span>, in the gdeliveryd folder); copies elsewhere are not used.</p>
      <button className="btn primary" onClick={() => void choose()} disabled={busy}>{busy ? <Loader2 size={14} className="spin" /> : <FolderOpen size={14} />} Choose zhuxian_question.data…</button>
      {error && <div className="se-problems" role="alert">{error}</div>}
    </div>
  </section>;

  const question = selected !== null ? questions[selected] ?? null : null;
  const errors = (problems ?? []).filter((problem) => problem.severity === "error").length;
  const left = questions.length % SET;
  const setLabel = (set: number) => {
    const days = daysOfSet.get(set);
    return days ? `day${days.length === 1 ? "" : "s"} ${days.join(", ")}` : "never runs";
  };
  const setAnswers = (answers: string[], key: number, label: string) => selected !== null && setQuestion(selected, { answers, key }, label);
  /** Moves an answer (and its correct mark) up or down. */
  const moveAnswer = (from: number, to: number) => {
    if (!question || to < 0 || to >= question.answers.length) return;
    const answers = [...question.answers];
    [answers[from], answers[to]] = [answers[to], answers[from]];
    const bit = (index: number) => (question.key >> index) & 1;
    let key = question.key & ~((1 << from) | (1 << to));
    key |= bit(from) << to;
    key |= bit(to) << from;
    setAnswers(answers, key, `answer-move:${Date.now()}`);
  };
  const removeAnswer = (at: number) => {
    if (!question) return;
    const answers = question.answers.filter((_, index) => index !== at);
    const low = question.key & ((1 << at) - 1);
    const high = (question.key >> (at + 1)) << at;
    setAnswers(answers, (low | high) & 0x3f, `answer-remove:${Date.now()}`);
  };

  return <section className="path-data-pane quiz-pane">
    <header className="path-data-head">
      <div>
        <h2>Quiz contest {dirty && <span className="tag warn">unsaved</span>}</h2>
        <div className="path-data-file-line">
          <span className="path-data-location mono muted small truncate" title={meta.path}>{meta.path}</span>
          <span className="path-data-badge"><b>Questions:</b> {count(questions.length)}</span>
          <span className="path-data-badge" title={`The server uses ${SET} questions a day, in sets of ${SET} in file order`}><b>Sets:</b> {count(sets)}</span>
          {left > 0 && <span className="path-data-badge" title="Questions after the last full set: never asked"><b>Unused:</b> {count(left)}</span>}
          <span className="path-data-badge"><b>Size:</b> {bytes(meta.size)}</span>
        </div>
      </div>
      <span className="quiz-view-tabs">
        <button className={"btn small" + (view === "questions" ? " active" : "")} onClick={() => setView("questions")}><ListChecks size={13} /> Questions</button>
        <button className={"btn small" + (view === "calendar" ? " active" : "")} onClick={() => setView("calendar")}><CalendarDays size={13} /> Calendar</button>
      </span>
      <button className="icon-btn" onClick={undo} disabled={busy || history.cursor <= 0} title="Undo (Ctrl+Z)"><Undo2 size={15} /></button>
      <button className="icon-btn" onClick={redo} disabled={busy || history.cursor + 1 >= history.states.length} title="Redo (Ctrl+Y)"><Redo2 size={15} /></button>
      <button className="btn" onClick={() => setProblemsOpen(true)} title="Problems (Ctrl+Shift+M)">{errors ? <CircleAlert size={13} className="danger-icon" /> : <TriangleAlert size={13} />} Problems{problems ? ` ${count(problems.length)}` : ""}</button>
      <button className="btn primary" onClick={saveCurrent} disabled={busy || !dirty}><Save size={13} /> Save</button>
      <label className="check"><input type="checkbox" checked={backup} onChange={(event) => { setBackup(event.target.checked); try { localStorage.setItem(BACKUP_KEY, event.target.checked ? "1" : "0"); } catch { /* optional */ } }} /> Backup</label>
      <button className="btn small nowrap" onClick={() => void saveAs()} disabled={busy}>Save as…</button>
      <button className="btn" onClick={() => void choose()} disabled={busy}><FolderOpen size={13} /> Open…</button>
    </header>
    {(error || note) && <div className={error ? "path-data-message error" : "path-data-message"} role={error ? "alert" : "status"}>
      {error ? <AlertTriangle size={14} /> : <Info size={14} />}{error || note} <button className="link" onClick={() => { setError(null); setNote(null); }}>Dismiss</button>
    </div>}
    {view === "calendar" ? <div className="quiz-calendar">
      <p className="muted small">At 19:00 every day the contest asks the {SET} questions of one set, one every 25 seconds (20 seconds to answer). The day of the month picks the set: day <i>d</i> uses set <i>d</i> while there are more sets than <i>d</i>, otherwise set <i>d</i> mod (number of sets), counting sets from 0 as the server does (shown here from 1).</p>
      {!sets ? <div className="path-data-message error"><CircleAlert size={14} /> Fewer than {SET} questions: there is no full set and gdeliveryd does not start.</div> : <div className="quiz-days">
        {Array.from({ length: 31 }, (_, offset) => offset + 1).map((day) => {
          const set = setOfDay(day, sets)!;
          const first = questions[set * SET];
          return <button key={day} className="quiz-day" onClick={() => { setSelected(set * SET); setQuery(""); setView("questions"); }} title={`Open set ${set + 1}`}>
            <span className="quiz-day-number">{day}</span>
            <b>Set {set + 1}</b>
            <span className="muted small truncate">{first?.text}</span>
          </button>;
        })}
      </div>}
      {sets > 0 && (() => {
        const unused = Array.from({ length: sets }, (_, set) => set).filter((set) => !daysOfSet.has(set));
        return <div className="quiz-calendar-notes">
          {unused.length > 0 && <p className="ai-warn small">Set{unused.length === 1 ? "" : "s"} {unused.map((set) => set + 1).join(", ")} never run{unused.length === 1 ? "s" : ""}: the day of the month only reaches sets 1–31 (and set 1 on days that are a multiple of the set count).</p>}
          {left > 0 && <p className="ai-warn small">The last {left} question{left === 1 ? " is" : "s are"} not part of a full set: never asked. Add {SET - left} more to make set {sets + 1}.</p>}
          {sets < 31 && <p className="muted small">{sets} sets for 31 days: days {sets}–31 repeat earlier sets.</p>}
        </div>;
      })()}
    </div> : <div className="quiz-body">
      <aside className="dyn-task-list">
        <div className="dyn-task-search"><Search size={13} /><input value={query} placeholder="Question or answer text, or number" onChange={(event) => setQuery(event.target.value)} />{query && <button className="icon-btn small" onClick={() => setQuery("")}><X size={12} /></button>}</div>
        <div className="dyn-task-rows" role="listbox" ref={rowsRef}>
          {visible.map(({ question: entry, index }) => <div key={index}>
            {!query && index % SET === 0 && <div className={"quiz-set-head" + (index >= sets * SET ? " unused" : "")}>{index >= sets * SET ? `Not a full set: never asked` : `Set ${index / SET + 1} · ${setLabel(index / SET)}`}</div>}
            <button role="option" aria-selected={index === selected} className={"dyn-task-row quiz-row" + (index === selected ? " selected" : "") + (index >= sets * SET ? " unused" : "")} onClick={() => setSelected(index)}>
              <span className="mono muted">{index + 1}</span>
              <span className="truncate">{entry.text || <span className="muted">(no text)</span>}</span>
              {flagged.get(index) === "error" ? <CircleAlert size={12} className="danger-icon" /> : flagged.has(index) ? <TriangleAlert size={12} className="ai-warn" /> : <span className="tag quiz-key">{correctLetters(entry) || "?"}</span>}
            </button>
          </div>)}
          {!visible.length && <div className="empty-note">No question matches.</div>}
        </div>
        <footer className="ai-list-foot">
          <span className="muted small">{count(visible.length)} of {count(questions.length)}</span>
          <span className="quiz-list-actions">
            <button className="icon-btn small" onClick={addQuestion} disabled={busy} title="New question below the selected one"><Plus size={13} /></button>
            <button className="icon-btn small" onClick={cloneSelected} disabled={busy || selected === null} title="Clone (Ctrl+D)"><Copy size={13} /></button>
            <button className="icon-btn small" onClick={() => selected !== null && moveTo(selected, selected - 1)} disabled={busy || !selected} title="Move up (Alt+↑)"><ArrowUp size={13} /></button>
            <button className="icon-btn small" onClick={() => selected !== null && moveTo(selected, selected + 1)} disabled={busy || selected === null || selected + 1 >= questions.length} title="Move down (Alt+↓)"><ArrowDown size={13} /></button>
            <button className="icon-btn small danger" onClick={deleteSelected} disabled={busy || selected === null} title="Delete (Del)"><Trash2 size={13} /></button>
          </span>
        </footer>
      </aside>
      <div className="dyn-task-form-scroll drops-form">
        {!question || selected === null ? <div className="empty-note">Select a question.</div> : <>
          {(problems ?? []).filter((problem) => problem.question === selected && (problem.severity !== "note" || problem.message.startsWith("The same"))).map((problem, index) => <div key={index} className={"path-data-message" + (problem.severity === "error" ? " error" : "")}>{problem.severity === "error" ? <CircleAlert size={13} /> : <TriangleAlert size={13} />} {problem.message}</div>)}
          <section className="dyn-section">
            <header><b>Question {selected + 1}</b><span className="muted small">{selected < sets * SET ? `set ${Math.floor(selected / SET) + 1}, question ${selected % SET + 1} of ${SET} · ${setLabel(Math.floor(selected / SET))}` : "not in a full set: never asked"}</span>
              <span className="spacer" />
              <label className="quiz-move small">Move to set <select className="task-form-select" value="" onChange={(event) => { const set = Number(event.target.value); const target = Math.min(questions.length - 1, set * SET + SET - 1); moveTo(selected, selected < target ? target : set * SET); }}>
                <option value="">…</option>
                {Array.from({ length: sets }, (_, set) => <option key={set} value={set}>{set + 1}</option>)}
              </select></label>
            </header>
            <div className="dyn-section-body">
              <TextInput value={question.text} multiline placeholder="The question" onCommit={(text) => setQuestion(selected, { text }, "text")} />
            </div>
          </section>
          <section className="dyn-section">
            <header><b>Answers</b><span className="muted small">{question.answers.length} of 6 · tick the correct one{question.answers.length ? "" : "s"}</span><span className="spacer" />
              <button className="btn small" disabled={question.answers.length >= 6} onClick={() => setAnswers([...question.answers, ""], question.key, `answer-add:${Date.now()}`)}><Plus size={12} /> Add an answer</button>
            </header>
            <div className="dyn-section-body">
              <table className="dyn-table quiz-answers">
                <thead><tr><th /><th>Correct</th><th>Answer</th><th /></tr></thead>
                <tbody>{question.answers.map((answer, index) => <tr key={index} className={question.key & (1 << index) ? "quiz-correct" : undefined}>
                  <td className="mono"><b>{LETTERS[index] ?? index + 1}</b></td>
                  <td><input type="checkbox" checked={!!(question.key & (1 << index))} disabled={index >= 6} onChange={(event) => setQuestion(selected, { key: event.target.checked ? question.key | (1 << index) : question.key & ~(1 << index) }, `key:${Date.now()}`)} title="Correct answer (players must pick exactly the ticked ones)" /></td>
                  <td><TextInput value={answer} placeholder="(empty: the server may crash)" onCommit={(text) => setAnswers(question.answers.map((entry, at) => at === index ? text : entry), question.key, `answer:${index}`)} /></td>
                  <td className="nowrap">
                    <button className="icon-btn small" disabled={index === 0} title="Move up" onClick={() => moveAnswer(index, index - 1)}><ArrowUp size={12} /></button>
                    <button className="icon-btn small" disabled={index + 1 >= question.answers.length} title="Move down" onClick={() => moveAnswer(index, index + 1)}><ArrowDown size={12} /></button>
                    <button className="icon-btn small danger" disabled={question.answers.length <= 2} title="Remove" onClick={() => removeAnswer(index)}><Trash2 size={12} /></button>
                  </td>
                </tr>)}</tbody>
              </table>
              <p className="muted small">Players must pick exactly the ticked answers; official questions have one. Each answer and the question need text (an empty one is a null string when the server sends it).</p>
            </div>
          </section>
        </>}
      </div>
    </div>}
    {problemsOpen && <div className="modal-backdrop" onMouseDown={() => setProblemsOpen(false)}>
      <div className="modal npcgen-problems-dialog" role="dialog" aria-label="Problems" onMouseDown={(event) => event.stopPropagation()}>
        <header className="modal-head"><h3>Problems</h3><span className="muted small">{count(errors)} error{errors === 1 ? "" : "s"}</span><span className="spacer" /><button className="icon-btn" onClick={() => setProblemsOpen(false)} aria-label="Close"><X size={16} /></button></header>
        <div className="npcgen-problems-list">
          {!problems ? <div className="empty-note"><Loader2 size={14} className="spin" /> Checking…</div> : !problems.length ? <div className="empty-note">No problems.</div> : problems.map((problem, index) => <button key={index} className={"npcgen-problem " + problem.severity} onClick={() => { if (problem.question !== null) { setSelected(problem.question); setQuery(""); setView("questions"); } setProblemsOpen(false); }}>
            {problem.severity === "error" ? <CircleAlert size={14} /> : problem.severity === "warning" ? <TriangleAlert size={14} /> : <Info size={14} />}
            <span>{problem.question !== null && <b>Question {problem.question + 1}</b>} {problem.message}</span>
          </button>)}
        </div>
        <footer className="modal-foot muted small">From gdeliveryd's question library and contest (questionlib.cpp, contestmanager.cpp).</footer>
      </div>
    </div>}
  </section>;
});
