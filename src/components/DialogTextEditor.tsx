import { useMemo, useState } from "react";
import { LockKeyhole, MessageSquareText, X } from "lucide-react";
import type { TalkDetail, TalkTextEdit } from "../elements/types";
import { GameText } from "./GameText";

interface Props {
  detail: TalkDetail;
  onApply: (edit: TalkTextEdit) => Promise<string | null>;
  onClose: () => void;
}

const draftOf = (detail: TalkDetail): TalkTextEdit => ({
  text: detail.text,
  windows: detail.windows.map((window) => ({ text: window.text, options: window.options.map((option) => option.text) })),
});

/** JavaScript string length is UTF-16 code units, which is what wchar[64] stores. */
const units = (value: string) => value.replace(/\r\n|\r|\n/g, "\r\n").length;

function TextPair({ label, original, value, onChange, fixed }: { label: string; original: string; value: string; onChange: (value: string) => void; fixed?: number }) {
  const used = units(value);
  const invalid = fixed !== undefined && used > fixed;
  return (
    <div className="dialog-translation-row">
      <div className="dialog-translation-original">
        <span className="dialog-translation-label">{label}</span>
        <GameText text={original || " "} />
      </div>
      <div className="dialog-translation-draft">
        <div className="dialog-translation-label">
          Translation
          {fixed !== undefined && <span className={invalid ? "limit bad" : "limit"}>{used} / {fixed}</span>}
        </div>
        <textarea value={value} onChange={(event) => onChange(event.target.value)} className={invalid ? "invalid" : ""} spellCheck={false} rows={Math.max(2, Math.min(7, value.split(/\r?\n/).length + 1))} />
        <div className="dialog-translation-preview"><GameText text={value || " "} /></div>
      </div>
    </div>
  );
}

/** Translation-only editor for TALK_PROC strings. It never exposes links or numeric control data. */
export function DialogTextEditor({ detail, onApply, onClose }: Props) {
  const [draft, setDraft] = useState<TalkTextEdit>(() => draftOf(detail));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const invalid = units(draft.text) > 64 || draft.windows.some((window) => window.options.some((option) => units(option) > 64));
  const changed = useMemo(() => JSON.stringify(draft) !== JSON.stringify(draftOf(detail)), [detail, draft]);

  const apply = async () => {
    if (!changed || invalid || busy) return;
    setBusy(true);
    setError(null);
    const problem = await onApply(draft);
    setBusy(false);
    if (problem) setError(problem.replace(/^Error: /, ""));
    else onClose();
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && !busy && onClose()}>
      <div className="modal dialog-text-editor" role="dialog" aria-modal="true" aria-label={`Translate NPC dialog ${detail.id}`} onKeyDown={(event) => { event.stopPropagation(); if (event.key === "Escape" && !busy) onClose(); }}>
        <header className="modal-head">
          <div>
            <h3><MessageSquareText size={16} /> Translate NPC dialog {detail.id}</h3>
            <p className="muted small">Edit visible text only. IDs, links, functions, parameters, counts and order remain locked.</p>
          </div>
          <button className="icon-btn" onClick={onClose} disabled={busy} title="Close" aria-label="Close"><X size={16} /></button>
        </header>
        <div className="dialog-translation-columns muted small"><span>Original</span><span>Editable text and game preview</span></div>
        <div className="modal-body scroll dialog-translation-body">
          <section className="dialog-translation-section">
            <div className="dialog-translation-section-head"><strong>Dialog title / prompt</strong><span className="locked"><LockKeyhole size={12} /> ID {detail.id}</span></div>
            <TextPair label="Original" original={detail.text} value={draft.text} fixed={64} onChange={(text) => setDraft({ ...draft, text })} />
          </section>
          {detail.windows.map((window, wi) => (
            <section className="dialog-translation-section" key={`${window.id}:${wi}`}>
              <div className="dialog-translation-section-head">
                <strong>Window {wi + 1}</strong>
                <span className="locked"><LockKeyhole size={12} /> ID {window.id} · parent {window.parent === 0xffffffff ? "root" : window.parent}</span>
              </div>
              <TextPair label="NPC text" original={window.text} value={draft.windows[wi].text} onChange={(text) => {
                const windows = draft.windows.slice();
                windows[wi] = { ...windows[wi], text };
                setDraft({ ...draft, windows });
              }} />
              {window.options.map((option, oi) => (
                <TextPair key={`${option.id}:${oi}`} label={`Player option ${oi + 1}`} original={option.text} value={draft.windows[wi].options[oi]} fixed={64} onChange={(text) => {
                  const windows = draft.windows.slice();
                  const options = windows[wi].options.slice();
                  options[oi] = text;
                  windows[wi] = { ...windows[wi], options };
                  setDraft({ ...draft, windows });
                }} />
              ))}
            </section>
          ))}
        </div>
        <footer className="modal-foot">
          <span className="muted small"><LockKeyhole size={12} /> Structure is read-only</span>
          {error && <span className="error-text">{error}</span>}
          <span className="spacer" />
          <button className="btn" onClick={onClose} disabled={busy}>Cancel</button>
          <button className="btn primary" onClick={apply} disabled={!changed || invalid || busy}>{busy ? "Applying…" : "Apply text"}</button>
        </footer>
      </div>
    </div>
  );
}
