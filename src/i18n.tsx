import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import type { LocaleCatalog, LocaleCatalogs } from "./elements/types";
import en from "./locales/en.json";

export type TranslationKey = keyof typeof en;
export type Locale = string;

export const englishMessages: Readonly<Record<TranslationKey, string>> = en;
const english = englishMessages;

export function normalizeLocale(value: string | null | undefined): Locale {
  const code = value?.trim().toLowerCase();
  return code && /^[a-z0-9_-]{1,32}$/.test(code) ? code : "en";
}

type Params = Record<string, string | number>;

function render(message: string, params?: Params): string {
  if (!params) return message;
  return message.replace(/\{([^}]+)\}/g, (whole, name: string) => (name in params ? String(params[name]) : whole));
}

export interface LocaleOption {
  code: string;
  name: string;
  path: string | null;
}

interface I18nValue {
  locale: Locale;
  locales: LocaleOption[];
  localeFiles: LocaleCatalogs | null;
  setLocale: (locale: Locale) => void;
  installLocales: (catalogs: LocaleCatalogs) => void;
  t: (key: TranslationKey, params?: Params) => string;
}

const I18nContext = createContext<I18nValue>({
  locale: "en",
  locales: [{ code: "en", name: "English", path: null }],
  localeFiles: null,
  setLocale: () => {},
  installLocales: () => {},
  t: (key, params) => render(english[key], params),
});

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>("en");
  const [localeFiles, setLocaleFiles] = useState<LocaleCatalogs | null>(null);
  const installLocales = useCallback((catalogs: LocaleCatalogs) => setLocaleFiles(catalogs), []);
  const setLocale = useCallback((code: Locale) => setLocaleState(normalizeLocale(code)), []);
  const catalogs = useMemo(() => {
    const loaded: Record<string, LocaleCatalog> = {};
    for (const catalog of localeFiles?.locales ?? []) loaded[normalizeLocale(catalog.code)] = catalog;
    return loaded;
  }, [localeFiles]);
  const locales = useMemo<LocaleOption[]>(() => [
    { code: "en", name: "English", path: null },
    ...Object.values(catalogs).sort((left, right) => left.name.localeCompare(right.name)).map(({ code, name, path }) => ({ code, name, path })),
  ], [catalogs]);
  const t = useCallback((key: TranslationKey, params?: Params) => {
    const translated = catalogs[locale]?.messages[key];
    return render(typeof translated === "string" && translated.trim().length ? translated : english[key], params);
  }, [catalogs, locale]);
  const value = useMemo(() => ({ locale, locales, localeFiles, setLocale, installLocales, t }), [installLocales, locale, localeFiles, locales, setLocale, t]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nValue {
  return useContext(I18nContext);
}

const SYSTEM_LABELS: Record<string, TranslationKey> = {
  File: "menu.file",
  Edit: "menu.edit",
  Tools: "menu.tools",
  About: "menu.about",
  Save: "common.save",
  "Save as…": "common.saveAs",
  "Save all": "common.saveAll",
  "Settings…": "common.settings",
  Undo: "common.undo",
  Redo: "common.redo",
  Problems: "common.problems",
  "Check problems…": "common.checkProblems",
  History: "common.history",
  Clone: "common.clone",
  "Delete…": "common.delete",
  Export: "common.export",
  "Export JSON…": "common.exportJson",
  "Import JSON…": "common.importJson",
};

/** Translates shared desktop-style labels while leaving format-specific labels untouched. */
export function systemLabel(label: string, t: I18nValue["t"]): string {
  const key = SYSTEM_LABELS[label];
  return key ? t(key) : label;
}
