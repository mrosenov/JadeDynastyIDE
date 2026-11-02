import { useEffect, useMemo, useState } from "react";
import { CircleAlert, Languages, Loader2, Search, X } from "lucide-react";
import { saveLocaleCatalog } from "../elements/api";
import type { LocaleCatalog, LocaleCatalogs } from "../elements/types";
import { englishMessages, useI18n, type TranslationKey } from "../i18n";

interface Props {
  catalog: LocaleCatalog | null;
  onSaved: (catalogs: LocaleCatalogs, code: string) => void;
  onClose: () => void;
}

const entries = Object.entries(englishMessages) as [TranslationKey, string][];

function placeholders(value: string): string[] {
  return [...new Set(Array.from(value.matchAll(/\{([^}]+)\}/g), (match) => match[1]))].sort();
}

function samePlaceholders(english: string, translated: string): boolean {
  return placeholders(english).join("\0") === placeholders(translated).join("\0");
}

export function LocaleEditor({ catalog, onSaved, onClose }: Props) {
  const { t } = useI18n();
  const initialMessages = useMemo<Record<string, string>>(() => Object.fromEntries(entries.flatMap(([key]) => catalog?.messages[key] !== undefined ? [[key, catalog.messages[key]]] : [])), [catalog]);
  const [name, setName] = useState(catalog?.name ?? "");
  const [code, setCode] = useState(catalog?.code ?? "");
  const [messages, setMessages] = useState<Record<string, string>>(initialMessages);
  const [search, setSearch] = useState("");
  const [saving, setSaving] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const normalizedCode = code.trim().toLowerCase();
  const validCode = /^(?!en$)[a-z0-9_-]{1,32}$/.test(normalizedCode);
  const changed = name !== (catalog?.name ?? "") || normalizedCode !== (catalog?.code ?? "") || entries.some(([key]) => (messages[key] ?? "") !== (initialMessages[key] ?? ""));
  const invalidKeys = entries.filter(([key, english]) => Boolean(messages[key]?.trim()) && !samePlaceholders(english, messages[key]));
  const translated = entries.filter(([key]) => Boolean(messages[key]?.trim())).length;
  const query = search.trim().toLocaleLowerCase();
  const visible = entries.filter(([key, english]) => !query || key.toLocaleLowerCase().includes(query) || english.toLocaleLowerCase().includes(query) || (messages[key] ?? "").toLocaleLowerCase().includes(query));

  const close = () => {
    if (!changed || window.confirm(t("localeEditor.discard"))) onClose();
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        close();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  const save = async () => {
    setSaving(true);
    setProblem(null);
    try {
      const catalogs = await saveLocaleCatalog({ originalCode: catalog?.code ?? null, code: normalizedCode, name, messages });
      onSaved(catalogs, normalizedCode);
    } catch (error) {
      setProblem(String(error));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="modal-backdrop locale-editor-backdrop" onMouseDown={(event) => event.target === event.currentTarget && close()}>
      <div className="modal locale-editor-dialog" role="dialog" aria-label={catalog ? t("localeEditor.editTitle", { name: catalog.name }) : t("localeEditor.createTitle")}>
        <header className="modal-head">
          <Languages size={17} />
          <h3>{catalog ? t("localeEditor.editTitle", { name: catalog.name }) : t("localeEditor.createTitle")}</h3>
          <span className="spacer" />
          <button className="icon-btn" onClick={close} aria-label={t("common.close")}><X size={18} /></button>
        </header>

        <div className="locale-editor-meta">
          <label className="settings-field">
            <span>{t("localeEditor.name")}</span>
            <input className="se-input" value={name} onChange={(event) => setName(event.target.value)} placeholder={t("localeEditor.namePlaceholder")} maxLength={80} autoFocus />
          </label>
          <label className="settings-field">
            <span>{t("localeEditor.code")}</span>
            <input className="se-input mono" value={code} onChange={(event) => setCode(event.target.value)} disabled={Boolean(catalog)} spellCheck={false} />
          </label>
          <p className="muted small">{t("localeEditor.codeHint")}</p>
          {!validCode && code.length > 0 && <p className="locale-editor-inline-error"><CircleAlert size={13} /> {t("localeEditor.invalidCode")}</p>}
        </div>

        <div className="locale-editor-toolbar">
          <label className="search-box locale-editor-search"><Search size={14} /><input className="se-input search" value={search} onChange={(event) => setSearch(event.target.value)} placeholder={t("localeEditor.search")} /></label>
          <span className="muted small">{t("localeEditor.progress", { translated, total: entries.length })}</span>
        </div>

        <div className="locale-editor-table scroll">
          <div className="locale-editor-heading"><span /><b>{t("localeEditor.english")}</b><b>{t("localeEditor.translation")}</b></div>
          {visible.map(([key, english]) => {
            const value = messages[key] ?? "";
            const mismatch = Boolean(value.trim()) && !samePlaceholders(english, value);
            return (
              <div className={"locale-editor-row" + (mismatch ? " invalid" : "")} key={key}>
                <div><code>{key}</code></div>
                <div className="locale-editor-source">{english}</div>
                <div>
                  <textarea className="se-input" rows={2} value={value} onChange={(event) => setMessages((current) => ({ ...current, [key]: event.target.value }))} placeholder={t("localeEditor.fallback")} />
                  {mismatch && <span className="locale-editor-placeholder-error">{t("localeEditor.placeholderError", { placeholders: placeholders(english).map((name) => `{${name}}`).join(", ") })}</span>}
                </div>
              </div>
            );
          })}
          {!visible.length && <div className="locale-editor-empty muted">{t("localeEditor.noMatches")}</div>}
        </div>

        <footer className="modal-foot">
          {problem && <span className="locale-editor-save-error"><CircleAlert size={14} /> {problem}</span>}
          <span className="spacer" />
          <button className="btn" onClick={close}>{t("common.cancel")}</button>
          <button className="btn primary" onClick={() => void save()} disabled={saving || !changed || !name.trim() || !validCode || invalidKeys.length > 0}>
            {saving ? <><Loader2 size={14} className="spin" /> {t("common.saving")}</> : t("localeEditor.save")}
          </button>
        </footer>
      </div>
    </div>
  );
}
