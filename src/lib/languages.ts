import type { Language } from "./api";

/** Human-readable name of each supported language. */
export const LANGUAGE_NAMES: Record<Language, string> = {
  en: "English",
  es: "Spanish",
};

/** Languages offered in pickers, in display order. */
export const LANGUAGE_OPTIONS: { value: Language; label: string }[] = [
  { value: "es", label: LANGUAGE_NAMES.es },
  { value: "en", label: LANGUAGE_NAMES.en },
];

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
