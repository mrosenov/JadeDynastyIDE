import { useEffect, useMemo, useState } from "react";
import { ArrowLeft, ChevronRight, CircleAlert, CornerUpLeft, LogOut, MessagesSquare, Pencil, RotateCcw, Zap } from "lucide-react";
import type { ListSummary, TalkDetail, TalkOption, TalkTextEdit, TalkWindow } from "../elements/types";
import { TALK_EXIT, TALK_RETURN, functionName, isFunction, paramNote, pathTo, rootWindow, windowsById } from "../elements/talk";
import { hex } from "../elements/format";
import { GameText } from "./GameText";
import { DialogTextEditor } from "./DialogTextEditor";

interface Props {
  detail: TalkDetail | null;
  lists: ListSummary[];
  canGoBack: boolean;
  onBack: () => void;
  /** Open a record that uses the dialog. */
  onFollow: (list: number, row: number, newTab?: boolean) => void;
  onEdit: (edit: TalkTextEdit) => Promise<string | null>;
}

type View = "conversation" | "outline";

/** An NPC dialog, played like in the game or laid out as a tree. */
export function DialogViewer({ detail, lists, canGoBack, onBack, onFollow, onEdit }: Props) {
  const [view, setView] = useState<View>("conversation");
  // Windows from the root to the one shown.
  const [path, setPath] = useState<number[]>([]);
  const [ended, setEnded] = useState(false);
  const [editing, setEditing] = useState(false);

  const byId = useMemo(() => windowsById(detail?.windows ?? []), [detail]);
  const root = detail ? rootWindow(detail.windows) : null;

  const restart = () => {
    setPath(root ? [root.id] : []);
    setEnded(false);
  };
  useEffect(restart, [detail]);

  if (!detail) {
    return (
      <section className="pane inspector">
        <div className="empty-note center">Pick a dialog to read it.</div>
      </section>
    );
  }

  const current = byId.get(path[path.length - 1] ?? -1) ?? null;
  const options = detail.windows.reduce((n, w) => n + w.options.length, 0);
  const prompt = detail.text.trim() && detail.text.trim() !== "RootNode" ? detail.text.trim() : null;

  const choose = (o: TalkOption) => {
    if (o.id === TALK_RETURN) {
      if (path.length > 1) setPath(path.slice(0, -1));
    } else if (o.id === TALK_EXIT) {
      setEnded(true);
    } else if (!isFunction(o) && byId.has(o.id)) {
      setPath([...path, o.id]);
    }
  };

  const jumpTo = (id: number) => {
    setPath(pathTo(detail.windows, id));
    setEnded(false);
    setView("conversation");
  };

  return (
    <>
    <section className="pane inspector dialog-viewer">
      <div className="inspector-head">
        <div className="inspector-title">
          {canGoBack && (
            <button className="back" onClick={onBack} title="Back to the previous record (Alt+←)">
              <ArrowLeft size={15} />
            </button>
          )}
          <span className="dialog-mark" aria-hidden>
            <MessagesSquare size={16} />
          </span>
          <h2 className="truncate">{prompt ?? (root?.text.split(/\r?\n/)[0] || <span className="muted">Untitled dialog</span>)}</h2>
          <span className="badge">NPC dialog</span>
          <span className="spacer" />
          <button className="btn" onClick={() => setEditing(true)}><Pencil size={13} /> Edit text</button>
        </div>
        <dl className="facts">
          <div>
            <dt>ID</dt>
            <dd className="mono">{detail.id}</dd>
          </div>
          <div>
            <dt>Index</dt>
            <dd className="mono">{detail.index}</dd>
          </div>
          <div>
            <dt>Windows</dt>
            <dd className="mono">{detail.windows.length}</dd>
          </div>
          <div>
            <dt>Options</dt>
            <dd className="mono">{options}</dd>
          </div>
          <div>
            <dt>File offset</dt>
            <dd className="mono">{hex(detail.offset, 8)}</dd>
          </div>
          <div>
            <dt>Size</dt>
            <dd className="mono">{detail.size} B</dd>
          </div>
        </dl>
        <div className="dialog-users">
          <span className="muted small">Opened by</span>
          {detail.users.length === 0 && <span className="muted small">no record of this file (its id_dialog fields)</span>}
          {detail.users.slice(0, 8).map((u) => (
            <button
              key={`${u.list}:${u.row}`}
              className="chip-link"
              onClick={(e) => onFollow(u.list, u.row, e.ctrlKey || e.metaKey)}
              onAuxClick={(e) => e.button === 1 && onFollow(u.list, u.row, true)}
              title={`${lists[u.list]?.name ?? `List ${u.list}`} · ID ${u.id} (Ctrl+click: new tab)`}
            >
              <span className="muted">{lists[u.list]?.name ?? `List ${u.list}`} ›</span> {u.name || `#${u.row}`}
            </button>
          ))}
          {detail.users.length > 8 && <span className="muted small">and {detail.users.length - 8} more</span>}
        </div>
      </div>

      <div className="inspector-body dialog-body">
        <div className="subhead">
          <div className="insp-tabs" role="tablist">
            <button role="tab" aria-selected={view === "conversation"} className={view === "conversation" ? "active" : ""} onClick={() => setView("conversation")}>
              Conversation
            </button>
            <button role="tab" aria-selected={view === "outline"} className={view === "outline" ? "active" : ""} onClick={() => setView("outline")}>
              Outline
            </button>
          </div>
          <span className="spacer" />
          {view === "conversation" && (
            <button className="link" onClick={restart} disabled={path.length <= 1 && !ended}>
              <RotateCcw size={13} /> Start over
            </button>
          )}
        </div>

        {view === "conversation" ? (
          <div className="dialog-scroll scroll">
            {path.length > 1 && (
              <nav className="dialog-trail" aria-label="Path">
                {path.map((id, i) => {
                  const w = byId.get(id);
                  // The option of the previous window that led here.
                  const via = i > 0 ? byId.get(path[i - 1])?.options.find((o) => o.id === id)?.text : null;
                  return (
                    <span key={i} className="dialog-step">
                      {i > 0 && <ChevronRight size={12} />}
                      <button className="link" onClick={() => setPath(path.slice(0, i + 1))} disabled={i === path.length - 1}>
                        {i === 0 ? "Start" : via || `window ${w?.id ?? id}`}
                      </button>
                    </span>
                  );
                })}
              </nav>
            )}

            {!current ? (
              <div className="empty-note">This dialog has no windows.</div>
            ) : (
              <>
                <div className="npc-bubble">
                  <GameText text={current.text || " "} />
                  <span className="window-id mono" title="Window ID">
                    #{current.id}
                  </span>
                </div>
                {ended ? (
                  <div className="dialog-ended">
                    <LogOut size={14} /> The conversation ends here.
                    <button className="link" onClick={restart}>
                      Start over
                    </button>
                  </div>
                ) : (
                  <div className="dialog-options">
                    {current.options.length === 0 && <div className="muted small">No options: the player closes the window.</div>}
                    {current.options.map((o, i) => (
                      <OptionButton key={i} option={o} exists={byId.has(o.id)} canReturn={path.length > 1} onChoose={() => choose(o)} />
                    ))}
                  </div>
                )}
              </>
            )}
          </div>
        ) : (
          <div className="dialog-scroll scroll">
            <Outline windows={detail.windows} onOpen={jumpTo} />
          </div>
        )}
      </div>
    </section>
    {editing && <DialogTextEditor detail={detail} onApply={onEdit} onClose={() => setEditing(false)} />}
    </>
  );
}

function OptionButton({ option: o, exists, canReturn, onChoose }: { option: TalkOption; exists: boolean; canReturn: boolean; onChoose: () => void }) {
  const fn = isFunction(o);
  const note = paramNote(o);
  if (o.id === TALK_RETURN || o.id === TALK_EXIT) {
    const back = o.id === TALK_RETURN;
    return (
      <button className="dialog-option function" onClick={onChoose} disabled={back && !canReturn}>
        {back ? <CornerUpLeft size={14} /> : <LogOut size={14} />}
        <span className="truncate">{o.text || functionName(o.id)}</span>
        <span className="option-target">{functionName(o.id)}</span>
      </button>
    );
  }
  if (fn) {
    return (
      <div className="dialog-option function static" title={`Function 0x${o.id.toString(16)} · param ${o.param}`}>
        <Zap size={14} />
        <span className="truncate">{o.text || functionName(o.id)}</span>
        <span className="option-target">
          {functionName(o.id)}
          {note ? ` · ${note}` : ""}
        </span>
      </div>
    );
  }
  return (
    <button className={"dialog-option" + (exists ? "" : " missing")} onClick={onChoose} disabled={!exists}>
      {exists ? <ChevronRight size={14} /> : <CircleAlert size={14} />}
      <span className="truncate">{o.text || <span className="muted">(no text)</span>}</span>
      <span className="option-target">{exists ? `window ${o.id}` : `missing window ${o.id}`}</span>
    </button>
  );
}

/** Every window as a tree, following the options that open child windows. */
function Outline({ windows, onOpen }: { windows: TalkWindow[]; onOpen: (id: number) => void }) {
  const byId = windowsById(windows);
  const root = rootWindow(windows);
  const seen = new Set<number>();

  const node = (w: TalkWindow, via: string | null, depth: number): React.ReactNode => {
    if (seen.has(w.id)) {
      return (
        <div key={`${w.id}-again-${depth}`} className="outline-node again" style={{ marginLeft: depth * 18 }}>
          <span className="muted small">↻ back to window {w.id}</span>
        </div>
      );
    }
    seen.add(w.id);
    const children = w.options.filter((o) => !isFunction(o));
    const functions = w.options.filter(isFunction);
    return (
      <div key={w.id}>
        <button className="outline-node" style={{ marginLeft: depth * 18 }} onClick={() => onOpen(w.id)} title="Open in the conversation">
          <span className="outline-head">
            {via !== null && <span className="outline-via">{via || "(no text)"}</span>}
            <span className="mono muted small">#{w.id}</span>
            {functions.map((o, i) => (
              <span key={i} className="outline-fn" title={o.text}>
                {functionName(o.id)}
              </span>
            ))}
          </span>
          <GameText text={w.text || " "} className="outline-text" />
        </button>
        {children.map((o) => {
          const child = byId.get(o.id);
          return child ? (
            node(child, o.text, depth + 1)
          ) : (
            <div key={`missing-${o.id}`} className="outline-node missing" style={{ marginLeft: (depth + 1) * 18 }}>
              <CircleAlert size={12} /> {o.text} → missing window {o.id}
            </div>
          );
        })}
      </div>
    );
  };

  const tree = root ? node(root, null, 0) : null;
  const unreachable = windows.filter((w) => !seen.has(w.id));
  return (
    <div className="outline">
      {tree}
      {unreachable.length > 0 && (
        <>
          <div className="outline-section muted small">Not reachable from the start ({unreachable.length})</div>
          {/* Windows shown under an earlier unreachable one are skipped. */}
          {unreachable.map((w) => (seen.has(w.id) ? null : node(w, null, 0)))}
        </>
      )}
    </div>
  );
}
