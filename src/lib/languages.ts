import type { Locale } from "../i18n/types";
import type { Language } from "./api";

/**
 * Every target language of `gpt-realtime-translate`, with the codes the API expects.
 *
 * Source: OpenAI's realtime translation guide,
 * https://developers.openai.com/cookbook/examples/voice_solutions/realtime_translation_guide
 * (the source language is auto-detected, so only targets are listed).
 */
export const LANGUAGES: readonly Language[] = [
  "es",
  "en",
  "pt",
  "fr",
  "ja",
  "ru",
  "zh",
  "de",
  "ko",
  "hi",
  "id",
  "vi",
  "it",
];

/** Languages most users of this app speak: shown first in pickers. */
const LIKELY: readonly Language[] = ["es", "en"];

const displayNames = new Map<Locale, Intl.DisplayNames | null>();

function namesFor(locale: Locale): Intl.DisplayNames | null {
  let names = displayNames.get(locale);
  if (names === undefined) {
    try {
      names = new Intl.DisplayNames([locale], {
        type: "language",
        fallback: "none",
      });
    } catch {
      names = null;
    }
    displayNames.set(locale, names);
  }
  return names;
}

/**
 * Name of a translator language in the panel language, for use inside a sentence ("Hear others
 * in Spanish"; lowercase in Spanish: "español"). Falls back to the code when the runtime has
 * no name for it.
 */
export function languageName(language: Language, locale: Locale): string {
  return namesFor(locale)?.of(language) ?? language.toUpperCase();
}

/** Name of a translator language as a standalone option: capitalized ("Español"). */
export function languageLabel(language: Language, locale: Locale): string {
  const name = languageName(language, locale);
  return name.charAt(0).toLocaleUpperCase(locale) + name.slice(1);
}

/**
 * Languages offered in pickers: the panel language first, then the other likely one (es/en), then
 * the rest alphabetically by their name in the panel language.
 */
export function languageOptions(
  locale: Locale,
): { value: Language; label: string }[] {
  const top = [...LIKELY].sort((a, b) =>
    a === locale ? -1 : b === locale ? 1 : 0,
  );
  const rest = LANGUAGES.filter((language) => !LIKELY.includes(language))
    .map((value) => ({ value, label: languageLabel(value, locale) }))
    .sort((a, b) => a.label.localeCompare(b.label, locale));
  return [
    ...top.map((value) => ({ value, label: languageLabel(value, locale) })),
    ...rest,
  ];
}

/** Translation direction: incoming = others → you, outgoing = you → others. */
export type Direction = "incoming" | "outgoing";
