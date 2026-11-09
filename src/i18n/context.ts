import { createContext, useContext } from "react";
import { createTranslator, type Translate, type Translator } from "./translate";
import type { UiLanguage } from "./types";

export interface I18n extends Translator {
  /** The `uiLanguage` setting currently applied. */
  preference: UiLanguage;
  /** Applies a new `uiLanguage` right away (persisting it is up to the caller). */
  setPreference: (preference: UiLanguage) => void;
}

/** Outside an `I18nProvider` the panel is English and the language cannot change. */
export const I18nContext = createContext<I18n>({
  ...createTranslator("en"),
  preference: "en",
  setPreference: () => undefined,
});

/** The current locale, translators, formatters and language preference. */
export function useI18n(): I18n {
  return useContext(I18nContext);
}

/** The message lookup for the current locale: `t("settings.title")`. */
export function useT(): Translate {
  return useContext(I18nContext).t;
}
