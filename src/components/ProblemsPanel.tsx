import { useEffect, useMemo, useState } from "react";
import { ChevronRight, CircleAlert, CircleX, Info, Loader2, RefreshCw, TriangleAlert, X } from "lucide-react";
import { listProblems } from "../elements/api";
import type { ListSummary, Problem, ProblemKind, ProblemReport, ProblemSeverity } from "../elements/types";
import { count } from "../elements/format";

interface Props {
  lists: ListSummary[];
  icon?: (pathId?: number | null) => string | undefined;
  /** Open what a problem is about: a record (at a field), a dialog or a list. */
  onOpen: (problem: Problem, newTab: boolean) => void;
  /** The error and warning counts, for the top bar badge. */
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

/** Things in the file the client may trip over, or that look wrong. */
export function ProblemsPanel({ lists, icon, onOpen, onCounts, onClose }: Props) {
  const [report, setReport] = useState<ProblemReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [shown, setShown] = useState<Set<ProblemSeverity>>(new Set(["error", "warning", "info"]));
  const [query, setQuery] = useState("");
  // Info groups start closed: they are about the layouts more than the data.
  const [open, setOpen] = useState<Set<ProblemKind>>(new Set());
  const [active, setActive] = useState<Problem | null>(null);

  const scan = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await listProblems();
      setReport(r);
      setOpen(new Set(r.kinds.filter((k) => k.severity !== "info" && k.count > 0).map((k) => k.kind)));
      const total = (s: ProblemSeverity) => r.kinds.filter((k) => k.severity === s).reduce((n, k) => n + k.count, 0);
      onCounts(total("error"), total("warning"));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    scan();
  }, []);

  const totals = useMemo(() => {
    const t: Record<ProblemSeverity, number> = { error: 0, warning: 0, info: 0 };
    for (const k of report?.kinds ?? []) t[k.severity] += k.count;
    return t;
  }, [report]);

  const q = query.trim().toLowerCase();
  const byKind = useMemo(() => {
    const m = new Map<ProblemKind, Problem[]>();
    for (const p of report?.problems ?? []) {
      const where = p.list !== undefined ? (lists[p.list]?.name ?? "") : "";
      if (q && ![p.name, p.message, where, String(p.id)].some((s) => s.toLowerCase().includes(q))) continue;
      m.set(p.kind, [...(m.get(p.kind) ?? []), p]);
    }
    return m;
  }, [report, q, lists]);

  const toggle = <T,>(set: Set<T>, value: T) => {
    const next = new Set(set);
    if (next.has(value)) next.delete(value);
    else next.add(value);
    return next;
  };

  const choose = (p: Problem, e: React.MouseEvent) => {
    setActive(p);
    onOpen(p, e.ctrlKey || e.metaKey || e.button === 1);
  };

  const kinds = (report?.kinds ?? []).filter((k) => shown.has(k.severity) && k.count > 0);

  return (
    <section className="pane search-panel problems-panel" onKeyDown={(e) => e.key === "Escape" && onClose()}>
      <div className="pane-head">
        <span className="pane-title">
          <CircleAlert size={14} /> Problems
        </span>
        <span className="spacer" />
        <button className="link" onClick={scan} disabled={busy} title="Scan the file again">
          {busy ? <Loader2 size={13} className="spin" /> : <RefreshCw size={13} />} Rescan
        </button>
        <button className="icon-btn small" onClick={onClose} title="Close (Esc)" aria-label="Close">
          <X size={15} />
        </button>
      </div>

      <div className="problems-tools">
        <div className="problems-filters" role="group" aria-label="Severities">
          {SEVERITIES.map(({ severity, label }) => (
            <button
              key={severity}
              className={"sev-toggle " + severity + (shown.has(severity) ? " on" : "")}
              onClick={() => setShown((s) => toggle(s, severity))}
              aria-pressed={shown.has(severity)}
              title={shown.has(severity) ? `Hide ${label.toLowerCase()}` : `Show ${label.toLowerCase()}`}
            >
              <SeverityIcon severity={severity} />
              {label}
              <b>{report ? count(totals[severity]) : "…"}</b>
            </button>
          ))}
        </div>
        <input className="sf-control" placeholder="Filter by name, ID, list or message…" value={query} onChange={(e) => setQuery(e.target.value)} spellCheck={false} />
        {report && !report.pathsChecked && (
          <div className="muted small">Paths and icons were not checked: set the game client folder in Settings to check them against path.data.</div>
        )}
        {error && (
          <div className="se-problems">
            <CircleAlert size={13} /> {error}
          </div>
        )}
      </div>

      <div className="search-results scroll">
        {!report && busy && <div className="empty-note">Scanning the file…</div>}
        {report && kinds.length === 0 && (
          <div className="empty-note">{totals.error + totals.warning + totals.info === 0 ? "No problems found." : "Nothing to show with these filters."}</div>
        )}
        {kinds.map((k) => {
          const items = byKind.get(k.kind) ?? [];
          if (q && items.length === 0) return null;
          const isOpen = open.has(k.kind) || q !== "";
          return (
            <div key={k.kind} className="search-group">
              <button className="search-group-head problems-head" onClick={() => setOpen((o) => toggle(o, k.kind))} title={k.description}>
                <ChevronRight size={14} className={"caret-icon" + (isOpen ? " open" : "")} />
                <SeverityIcon severity={k.severity} />
                <span className="truncate">{k.title}</span>
                <span className="spacer" />
                <span className="muted small">
                  {q ? `${items.length} / ` : ""}
                  {count(k.count)}
                </span>
              </button>
              {isOpen && (
                <>
                  <div className="problems-desc muted small">{k.description}</div>
                  {items.map((p, i) => {
                    const where = p.list !== undefined ? (lists[p.list]?.name ?? `List ${p.list}`) : "NPC dialog";
                    return (
                      <button
                        key={i}
                        className={"search-hit problem-hit" + (active === p ? " active" : "")}
                        onClick={(e) => choose(p, e)}
                        onAuxClick={(e) => e.button === 1 && choose(p, e)}
                        title={p.row === undefined ? "Open the list" : "Open (Ctrl+click: new tab)"}
                      >
                        <span className="find-icon">{icon?.(p.icon) ? <img src={icon(p.icon)} alt="" draggable={false} /> : null}</span>
                        <span className="search-hit-main">
                          <span className="truncate">
                            {p.name || <span className="muted">Unnamed</span>}
                            {p.row !== undefined && <span className="muted small"> · {where}</span>}
                          </span>
                          <span className="problem-message">{p.message}</span>
                        </span>
                        <span className="mono muted small">{p.row !== undefined && p.id ? p.id : ""}</span>
                      </button>
                    );
                  })}
                  {items.length < k.count && !q && (
                    <div className="search-note muted small">Showing the first {count(items.length)} of {count(k.count)}.</div>
                  )}
                </>
              )}
            </div>
          );
        })}
        {report && <div className="problems-foot muted small">Scanned in {report.elapsedMs} ms.</div>}
      </div>
    </section>
  );
}
