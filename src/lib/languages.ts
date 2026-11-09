import type { Translate } from "../i18n/translate";
import type { Language } from "./api";

/** Translator languages, in picker order. */
const LANGUAGES: readonly Language[] = ["es", "en"];

/** Name of a translator language for use inside a sentence ("Hear others in Spanish"). */
export function languageName(language: Language, t: Translate): string {
  return t(`languages.name.${language}`);
}

/** Languages offered in pickers, in display order. */
export function languageOptions(
  t: Translate,
): { value: Language; label: string }[] {
  return LANGUAGES.map((value) => ({
    value,
    label: t(`languages.option.${value}`),
  }));
}

/**
 * The language a direction translates from, given the language it translates into.
 *
 * v1 only supports Spanish ⇄ English, so the source is always the other one.
 */
export function sourceLanguageFor(target: Language): Language {
  return target === "es" ? "en" : "es";
}

/** Translation direction: incoming = others → you, outgoing = you → others. */
export type Direction = "incoming" | "outgoing";
