import { useEffect, useMemo, useState, type ReactNode } from "react";
import { I18nContext, type I18n } from "./context";
import {
  createTranslator,
  formattingLocale,
  resolveLocale,
  systemLanguages,
} from "./translate";
import type { UiLanguage } from "./types";

export interface I18nProviderProps {
  /** Initial `uiLanguage` (from the saved settings). Defaults to following the OS. */
  preference?: UiLanguage;
  /** OS languages, most preferred first. Defaults to what the WebView reports. */
  languages?: readonly string[];
  children: ReactNode;
}

/** Provides the panel language to `useT()`/`useI18n()` and keeps `<html lang>` in sync. */
function I18nProvider({
  preference: initial = "system",
  languages,
  children,
}: I18nProviderProps) {
  const [preference, setPreference] = useState<UiLanguage>(initial);
  const osLanguages = useMemo(
    () => languages ?? systemLanguages(),
    [languages],
  );
  const locale = resolveLocale(preference, osLanguages);
  const formatTag = formattingLocale(locale, osLanguages);

  const value = useMemo<I18n>(
    () => ({
      ...createTranslator(locale, formatTag),
      preference,
      setPreference,
    }),
    [locale, formatTag, preference],
  );

  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export default I18nProvider;
