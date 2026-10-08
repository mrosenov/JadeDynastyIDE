import { useCallback, useEffect, useMemo, useState } from "react";
import { ChevronRight, CircleAlert, CircleX, Info, Loader2, RefreshCw, TriangleAlert, X } from "lucide-react";
import { getTaskProblems } from "../elements/api";
import type { ProblemSeverity, TaskEditState, TaskProblem, TaskProblemKind, TaskProblemReport } from "../elements/types";
import { count } from "../elements/format";

interface Props {
  /** The edit state: the task set is scanned again whenever it changes. */
  edits: TaskEditState;
  /** The background task index is complete; scanning needs it. */
  indexReady: boolean;
  /** Selects the task a problem is about. */
  onOpen: (problem: TaskProblem) => void;
  /** The error and warning counts, for the header button. */
  onCounts: (errors: number, warnings: number) => void;
  onClose: () => void;
}

const SEVERITIES: { severity: ProblemSeverity; label: string }[] = [
  { severity: "error", label: "Errors" },
  { severity: "warning", label: "Warnings" },
  { severity: "info", label: "Info" },
];

function SeverityIcon({ severity, size = 14 }: { severity: ProblemSeverity; size?: number }) {
  if (severity === "error") return <CircleX size={size} className="sev error" />;
  if (severity === "warning") return <TriangleAlert size={size} className="sev warning" />;
  return <Info size={size} className="sev info" />;
}

const where = (problem: TaskProblem) => problem.root === undefined
  ? `pack ${problem.pack + 1}`
  : `pack ${problem.pack + 1} · root ${problem.root + 1}${problem.path.length ? ` › subquest ${problem.path.map((index) => index + 1).join(".")}` : ""}`;

/** Things in the task set the game may trip over, or that look wrong. */
export function TaskProblemsPanel({ edits, indexReady, onOpen, onCounts, onClose }: Props) {
  const [report, setReport] = useState<TaskProblemReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [shown, setShown] = useState<Set<ProblemSeverity>>(new Set(["error", "warning", "info"]));
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<Set<TaskProblemKind> | null>(null);
  const [active, setActive] = useState<TaskProblem | null>(null);

  const scan = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const next = await getTaskProblems();
      setReport(next);
      // Info groups start closed; a rescan after an edit keeps what was opened.
      setOpen((current) => current ?? new Set(next.kinds.filter((kind) => kind.severity !== "info" && kind.count > 0).map((kind) => kind.kind)));
      const total = (severity: ProblemSeverity) => next.kinds.filter((kind) => kind.severity === severity).reduce((sum, kind) => sum + kind.count, 0);
      onCounts(total("error"), total("warning"));
    } catch (problem) {
      setError(String(problem).replace(/^Error: /, ""));
    } finally {
      setBusy(false);
    }
  }, [onCounts]);

  // Scans read the index only, so every edit rescans at once.
  useEffect(() => {
    if (indexReady) void scan();
  }, [edits, indexReady, scan]);

  const totals = useMemo(() => {
    const result: Record<ProblemSeverity, number> = { error: 0, warning: 0, info: 0 };
    for (const kind of report?.kinds ?? []) result[kind.severity] += kind.count;
    return result;
  }, [report]);

  const needle = query.trim().toLowerCase();
  const byKind = useMemo(() => {
    const result = new Map<TaskProblemKind, TaskProblem[]>();
    for (const problem of report?.problems ?? []) {
      if (needle && ![problem.name, problem.message, problem.field ?? "", String(problem.id), String(problem.target ?? "")].some((text) => text.toLowerCase().includes(needle))) continue;
      result.set(problem.kind, [...(result.get(problem.kind) ?? []), problem]);
    }
    return result;
  }, [report, needle]);

  const toggle = <T,>(set: Set<T>, value: T) => {
    const next = new Set(set);
    if (next.has(value)) next.delete(value);
    else next.add(value);
    return next;
  };

  const kinds = (report?.kinds ?? []).filter((kind) => shown.has(kind.severity) && kind.count > 0);

  return (
    <section className="pane search-panel problems-panel task-problems-panel" onKeyDown={(event) => event.key === "Escape" && onClose()}>
      <div className="pane-head">
        <span className="pane-title"><CircleAlert size={14} /> Task problems</span>
        <span className="spacer" />
        <button className="link" onClick={() => void scan()} disabled={busy || !indexReady} title="Scan the task set again">
          {busy ? <Loader2 size={13} className="spin" /> : <RefreshCw size={13} />} Rescan
        </button>
        <button className="icon-btn small" onClick={onClose} title="Close (Esc)" aria-label="Close"><X size={15} /></button>
      </div>

      <div className="problems-tools">
        <div className="problems-filters" role="group" aria-label="Severities">
          {SEVERITIES.map(({ severity, label }) => (
            <button
              key={severity}
              className={"sev-toggle " + severity + (shown.has(severity) ? " on" : "")}
              onClick={() => setShown((current) => toggle(current, severity))}
              aria-pressed={shown.has(severity)}
              title={shown.has(severity) ? `Hide ${label.toLowerCase()}` : `Show ${label.toLowerCase()}`}
            >
              <SeverityIcon severity={severity} />
              {label}
              <b>{report ? count(totals[severity]) : "…"}</b>
            </button>
          ))}
        </div>
        <input className="sf-control" placeholder="Filter by name, task ID, field or message…" value={query} onChange={(event) => setQuery(event.target.value)} spellCheck={false} />
        {report && !report.elementsChecked && <div className="muted small">Item, monster and object IDs were not checked: open the matching elements.data to check them.</div>}
        {error && <div className="se-problems"><CircleAlert size={13} /> {error}</div>}
      </div>

      <div className="search-results scroll">
        {!indexReady && <div className="empty-note">Indexing quests… The scan starts when every subquest is indexed.</div>}
        {indexReady && !report && busy && <div className="empty-note">Scanning the task set…</div>}
        {report && kinds.length === 0 && (
          <div className="empty-note">{totals.error + totals.warning + totals.info === 0 ? "No problems found." : "Nothing to show with these filters."}</div>
        )}
        {kinds.map((kind) => {
          const items = byKind.get(kind.kind) ?? [];
          if (needle && items.length === 0) return null;
          const isOpen = !!open?.has(kind.kind) || needle !== "";
          return (
            <div key={kind.kind} className="search-group">
              <button className="search-group-head problems-head" onClick={() => setOpen((current) => toggle(current ?? new Set(), kind.kind))} title={kind.description}>
                <ChevronRight size={14} className={"caret-icon" + (isOpen ? " open" : "")} />
                <SeverityIcon severity={kind.severity} />
                <span className="truncate">{kind.title}</span>
                <span className="spacer" />
                <span className="muted small">{needle ? `${items.length} / ` : ""}{count(kind.count)}</span>
              </button>
              {isOpen && <>
                <div className="problems-desc muted small">{kind.description}</div>
                {items.map((problem, index) => (
                  <button
                    key={index}
                    className={"search-hit problem-hit" + (active === problem ? " active" : "")}
                    onClick={() => { setActive(problem); onOpen(problem); }}
                    disabled={problem.root === undefined}
                    title={problem.root === undefined ? undefined : "Select this task"}
                  >
                    <span className="find-icon" />
                    <span className="search-hit-main">
                      <span className="truncate">{problem.name || <span className="muted">Unnamed</span>}<span className="muted small"> · {where(problem)}</span></span>
                      <span className="problem-message">{problem.message}</span>
                    </span>
                    <span className="mono muted small">{problem.root !== undefined ? problem.id : ""}</span>
                  </button>
                ))}
                {items.length < kind.count && !needle && <div className="search-note muted small">Showing the first {count(items.length)} of {count(kind.count)}.</div>}
              </>}
            </div>
          );
        })}
        {report && <div className="problems-foot muted small">Scanned {count(report.tasks)} tasks in {report.elapsedMs} ms.</div>}
      </div>
    </section>
  );
}
