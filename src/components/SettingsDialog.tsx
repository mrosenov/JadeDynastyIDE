import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { ChevronRight, CircleAlert, CircleCheck, Database, Eye, EyeOff, FilePlus2, FolderSearch, KeyRound, Languages, Loader2, Monitor, Moon, Package, Pencil, RefreshCw, Settings as SettingsIcon, Sparkles, Sun, X } from "lucide-react";
import { getLocaleCatalogs, inspectClient, saveSettings } from "../elements/api";
import type { ClientInfo, DataFile, LocaleCatalog, SettingsView, Theme } from "../elements/types";
import { bytes } from "../elements/format";
import { applyTheme } from "../theme";
import { normalizeLocale, useI18n, type Locale } from "../i18n";
import { LocaleEditor } from "./LocaleEditor";

interface Props {
  view: SettingsView;
  onSaved: (view: SettingsView) => void;
  onOpenFile: (file: DataFile) => void;
  onClose: () => void;
}

/** App settings: the game client folder and what was found in it. */
export function SettingsDialog({ view, onSaved, onOpenFile, onClose }: Props) {
  const { installLocales, localeFiles, locales, t } = useI18n();
  const defaultAiEndpoint = "https://api.openai.com/v1/responses";
  const [dir, setDir] = useState(view.settings.clientDir ?? "");
  const [openOnStart, setOpenOnStart] = useState(view.settings.openOnStart);
  const [theme, setTheme] = useState<Theme>(view.settings.theme);
  const savedLanguage = normalizeLocale(view.settings.language);
  const [language, setLanguage] = useState<Locale>(locales.some((locale) => locale.code === savedLanguage) ? savedLanguage : "en");
  const [localeBusy, setLocaleBusy] = useState(false);
  const [localeProblem, setLocaleProblem] = useState<string | null>(null);
  const [localeEditor, setLocaleEditor] = useState<{ catalog: LocaleCatalog | null } | null>(null);
  const [aiEndpoint, setAiEndpoint] = useState(view.settings.aiEndpoint ?? defaultAiEndpoint);
  const [aiModel, setAiModel] = useState(view.settings.aiModel ?? "");
  const [aiApiKey, setAiApiKey] = useState(view.settings.aiApiKey ?? "");
  const [showAiKey, setShowAiKey] = useState(false);
  const [client, setClient] = useState<ClientInfo | null>(view.client);
  const [problem, setProblem] = useState<string | null>(view.clientError);
  const [checking, setChecking] = useState(false);
  const [saving, setSaving] = useState(false);
  const keepPreviewedTheme = useRef(false);

  // Preview theme choices immediately, but restore the saved choice when cancelled.
  useEffect(() => applyTheme(theme), [theme]);
  useEffect(
    () => () => {
      if (!keepPreviewedTheme.current) applyTheme(view.settings.theme);
    },
    [view.settings.theme],
  );

  // Check the folder as it is typed or picked.
  useEffect(() => {
    const trimmed = dir.trim();
    if (!trimmed) {
      setClient(null);
      setProblem(null);
      return;
    }
    let cancelled = false;
    setChecking(true);
    const timer = setTimeout(() => {
      inspectClient(trimmed)
        .then((info) => {
          if (cancelled) return;
          setClient(info);
          setProblem(null);
        })
        .catch((e) => {
          if (cancelled) return;
          setClient(null);
          setProblem(String(e));
        })
        .finally(() => !cancelled && setChecking(false));
    }, 300);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [dir]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        if (!localeEditor) onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [localeEditor, onClose]);

  const browse = async () => {
    const picked = await open({ directory: true, multiple: false, title: "Pick the game client folder" });
    if (typeof picked === "string") setDir(picked);
  };

  const reloadLocales = async () => {
    setLocaleBusy(true);
    setLocaleProblem(null);
    try {
      const catalogs = await getLocaleCatalogs();
      installLocales(catalogs);
      if (language !== "en" && !catalogs.locales.some((locale) => locale.code === language)) setLanguage("en");
      return catalogs;
    } catch (problem) {
      setLocaleProblem(String(problem));
      return null;
    } finally {
      setLocaleBusy(false);
    }
  };

  const openLocaleFolder = async () => {
    const catalogs = localeFiles ?? await reloadLocales();
    if (catalogs) await revealItemInDir(catalogs.templatePath).catch((problem) => setLocaleProblem(String(problem)));
  };

  const save = async () => {
    setSaving(true);
    try {
      const next = await saveSettings({
        clientDir: dir.trim() || null,
        openOnStart,
        theme,
        language,
        aiEndpoint: aiEndpoint.trim() || null,
        aiModel: aiModel.trim() || null,
        aiApiKey: aiApiKey.trim() || null,
      });
      keepPreviewedTheme.current = true;
      onSaved(next);
      onClose();
    } catch (e) {
      setProblem(String(e));
    } finally {
      setSaving(false);
    }
  };

  const changed =
    (view.settings.clientDir ?? "") !== dir.trim() ||
    view.settings.openOnStart !== openOnStart ||
    view.settings.theme !== theme ||
    normalizeLocale(view.settings.language) !== language ||
    (view.settings.aiEndpoint ?? defaultAiEndpoint) !== aiEndpoint.trim() ||
    (view.settings.aiModel ?? "") !== aiModel.trim() ||
    (view.settings.aiApiKey ?? "") !== aiApiKey.trim();
  const aiStarted = Boolean(aiModel.trim() || aiApiKey.trim());
  const aiComplete = Boolean(aiEndpoint.trim() && aiModel.trim() && aiApiKey.trim());
  const canSave = !saving && !checking && (!dir.trim() || client !== null);
  const selectedLocale = localeFiles?.locales.find((catalog) => catalog.code === language) ?? null;

  return (
    <>
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal settings-dialog" role="dialog" aria-label={t("settings.title")}>
        <header className="modal-head">
          <SettingsIcon size={17} />
          <h3>{t("settings.title")}</h3>
          <span className="spacer" />
          <button className="icon-btn" onClick={onClose} aria-label={t("common.close")}>
            <X size={18} />
          </button>
        </header>

        <div className="modal-body scroll">
          <section className="settings-section">
            <h4>{t("settings.appearance")}</h4>
            <div className="settings-theme" role="radiogroup" aria-label={t("settings.theme")}>
              {([
                ["system", Monitor, t("settings.themeSystem")],
                ["light", Sun, t("settings.themeLight")],
                ["dark", Moon, t("settings.themeDark")],
              ] as const).map(([value, Icon, label]) => (
                <button type="button" role="radio" aria-checked={theme === value} className={theme === value ? "active" : ""} onClick={() => setTheme(value)} key={value}>
                  <Icon size={15} /> {label}
                </button>
              ))}
            </div>
            <p className="muted small settings-theme-note">{t("settings.themeNote")}</p>
          </section>

          <section className="settings-section">
            <h4><Languages size={15} /> {t("settings.language")}</h4>
            <label className="settings-field settings-language">
              <select className="se-input" value={language} onChange={(event) => setLanguage(normalizeLocale(event.target.value))}>
                {locales.map((locale) => <option value={locale.code} key={locale.code}>{locale.code === "en" ? t("language.en") : locale.name}</option>)}
              </select>
            </label>
            <p className="muted small settings-theme-note">{t("settings.languageNote")}</p>
            <div className="settings-locale-actions">
              <button className="btn" onClick={() => setLocaleEditor({ catalog: null })}><FilePlus2 size={14} /> {t("settings.createLocale")}</button>
              <button className="btn" onClick={() => selectedLocale && setLocaleEditor({ catalog: selectedLocale })} disabled={!selectedLocale}><Pencil size={14} /> {t("settings.editLocale")}</button>
              <button className="btn" onClick={() => void openLocaleFolder()} disabled={localeBusy}><FolderSearch size={14} /> {t("settings.openLocaleFolder")}</button>
              <button className="btn" onClick={() => void reloadLocales()} disabled={localeBusy}>{localeBusy ? <Loader2 size={14} className="spin" /> : <RefreshCw size={14} />} {t("settings.reloadLocales")}</button>
              {localeFiles && <span className="mono muted small truncate" title={localeFiles.directory}>{localeFiles.directory}</span>}
            </div>
            {(localeProblem || localeFiles?.errors.length) && <div className="settings-status bad"><CircleAlert size={14} /><div>{localeProblem || <>{t("settings.localeErrors")}<ul>{localeFiles!.errors.map((error) => <li key={error}>{error}</li>)}</ul></>}</div></div>}
          </section>

          <section className="settings-section">
            <details className="settings-ai">
              <summary>
                <ChevronRight size={14} className="settings-ai-caret" />
                <Sparkles size={15} />
                <b>AI layout analysis</b>
                <span className={"settings-ai-state" + (aiComplete ? " ready" : "")}>{aiComplete ? "Configured" : "Not configured"}</span>
              </summary>
              <div className="settings-ai-body">
                <p className="muted small">
                  Compare an unfamiliar list with a trusted elements.data and prepare a schema draft. The Analyze button appears only when the endpoint, model and API key are set.
                </p>
                <label className="settings-field">
                  <span>API endpoint</span>
                  <input className="se-input mono" value={aiEndpoint} onChange={(e) => setAiEndpoint(e.target.value)} placeholder={defaultAiEndpoint} spellCheck={false} />
                </label>
                <label className="settings-field">
                  <span>Model</span>
                  <input className="se-input mono" value={aiModel} onChange={(e) => setAiModel(e.target.value)} placeholder="Model name supported by the endpoint" spellCheck={false} />
                </label>
                <label className="settings-field">
                  <span>API key</span>
                  <span className="settings-secret">
                    <KeyRound size={14} />
                    <input className="se-input mono" type={showAiKey ? "text" : "password"} value={aiApiKey} onChange={(e) => setAiApiKey(e.target.value)} placeholder="API key" spellCheck={false} autoComplete="off" />
                    <button type="button" className="icon-btn" onClick={() => setShowAiKey((shown) => !shown)} aria-label={showAiKey ? "Hide API key" : "Show API key"}>
                      {showAiKey ? <EyeOff size={15} /> : <Eye size={15} />}
                    </button>
                  </span>
                </label>
                {aiStarted && !aiComplete && <div className="settings-status bad"><CircleAlert size={14} /> Layout analysis stays disabled until all three fields are complete.</div>}
                {aiComplete && <div className="settings-status ok"><CircleCheck size={14} /> Layout analysis will be available in the schema editor.</div>}
                <p className="muted small settings-ai-note">The API key is stored in JD IDE's local settings and sent only to this endpoint.</p>
              </div>
            </details>
          </section>

          <section className="settings-section">
            <h4>{t("settings.gameClient")}</h4>
            <p className="muted small">
              The client folder holds the data files (element\data) and the resource packages (element\*.pck). JD IDE
              reads path.data and the item icons from it.
            </p>
            <div className="settings-row">
              <input
                className="se-input mono"
                value={dir}
                placeholder="e.g. E:\Games\ForsakenJD"
                onChange={(e) => setDir(e.target.value)}
                spellCheck={false}
              />
              <button className="btn" onClick={browse}>
                <FolderSearch size={15} /> {t("common.browse")}
              </button>
              {dir && (
                <button className="btn" onClick={() => setDir("")} title="Forget the client folder">
                  {t("common.clear")}
                </button>
              )}
            </div>

            {checking && (
              <div className="settings-status muted">
                <Loader2 size={14} className="spin" /> Checking the folder…
              </div>
            )}
            {!checking && problem && (
              <div className="settings-status bad">
                <CircleAlert size={14} /> {problem}
              </div>
            )}
            {!checking && client && (
              <>
                <div className="settings-status ok">
                  <CircleCheck size={14} />
                  Client found: {client.dataFiles.length} data files, {client.packages.length} packages
                  {client.hasPathData ? ", path.data" : ""}
                  {client.hasItemIcons ? ", item icons" : ""}
                </div>
                <label className="check settings-check">
                  <input
                    type="checkbox"
                    checked={openOnStart}
                    onChange={(e) => setOpenOnStart(e.target.checked)}
                    disabled={!client.elementsPath}
                  />
                  Open the client's elements.data when JD IDE starts
                </label>

                <details className="settings-files">
                  <summary>
                    <ChevronRight size={14} className="settings-files-caret" />
                    <Database size={14} />
                    <b>Data files</b>
                    <span className="settings-files-count">{client.dataFiles.length}</span>
                    <span className="muted mono truncate">{client.elementDir}\data</span>
                  </summary>
                  <div className="settings-table">
                    {client.dataFiles.map((f) => (
                      <div className="settings-file" key={f.path}>
                        <span className="mono truncate">{f.name}</span>
                        <span className="muted small">{f.kind}</span>
                        <span className="muted small mono">{bytes(f.size)}</span>
                        {f.supported ? (
                          <button
                            className="link"
                            onClick={() => {
                              onOpenFile(f);
                              onClose();
                            }}
                          >
                            Open
                          </button>
                        ) : (
                          <span className="size-badge differs" title="Reading this kind of file is planned for a later version">
                            later
                          </span>
                        )}
                      </div>
                    ))}
                  </div>
                </details>

                <details className="settings-files">
                  <summary>
                    <ChevronRight size={14} className="settings-files-caret" />
                    <Package size={14} />
                    <b>Packages</b>
                    <span className="settings-files-count">{client.packages.length}</span>
                    <span className="muted mono truncate">{client.elementDir}</span>
                  </summary>
                  <div className="settings-table">
                    {client.packages.map((p) => (
                      <div className="settings-file" key={p.path}>
                        <span className="mono truncate">{p.name}</span>
                        <span className="muted small">{p.parts > 1 ? `${p.parts} parts` : ""}</span>
                        <span className="muted small mono">{bytes(p.size)}</span>
                        <span />
                      </div>
                    ))}
                  </div>
                </details>
              </>
            )}
          </section>
        </div>

        <footer className="modal-foot">
          <span className="muted small">{t("settings.savedInConfig")}</span>
          <span className="spacer" />
          <button className="btn" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button className="btn primary" onClick={save} disabled={!canSave || !changed}>
            {saving ? t("common.saving") : t("common.save")}
          </button>
        </footer>
      </div>
    </div>
    {localeEditor && <LocaleEditor
      catalog={localeEditor.catalog}
      onClose={() => setLocaleEditor(null)}
      onSaved={(catalogs, code) => {
        installLocales(catalogs);
        setLanguage(code);
        setLocaleEditor(null);
      }}
    />}
    </>
  );
}
