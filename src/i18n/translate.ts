import { Fragment, createElement, type ReactNode } from "react";
import { en } from "./en";
import { es } from "./es";
import type {
  Locale,
  MessageKey,
  MessageParams,
  Messages,
  Plural,
  UiLanguage,
} from "./types";

export const DICTIONARIES: Record<Locale, Messages> = { en, es };

/** Locales offered by the language selector, in display order. */
export const LOCALES: readonly Locale[] = ["es", "en"];

/** Native name of each locale, shown in the language selector whatever the panel language. */
export const LOCALE_NATIVE_NAMES: Record<Locale, string> = {
  en: "English",
  es: "Español",
};

/** The OS languages as the WebView reports them (Windows display language first). */
export function systemLanguages(): readonly string[] {
  if (typeof navigator === "undefined") return [];
  if (navigator.languages.length > 0) return navigator.languages;
  return navigator.language ? [navigator.language] : [];
}

/** Panel locale for an OS language list: Spanish for any `es*`, English otherwise. */
export function systemLocale(languages: readonly string[]): Locale {
  const first = languages[0]?.toLowerCase() ?? "";
  return first === "es" || first.startsWith("es-") ? "es" : "en";
}

/** Resolves the `uiLanguage` setting to the locale the panel uses. */
export function resolveLocale(
  preference: UiLanguage,
  languages: readonly string[],
): Locale {
  return preference === "system" ? systemLocale(languages) : preference;
}

/**
 * BCP 47 tag for number/currency/date formatting: the OS language when it is a variant of
 * `locale` (so `es-AR` keeps its own conventions), otherwise a sensible default.
 */
export function formattingLocale(
  locale: Locale,
  languages: readonly string[],
): string {
  const match = languages.find(
    (tag) =>
      tag.toLowerCase() === locale ||
      tag.toLowerCase().startsWith(`${locale}-`),
  );
  return match ?? (locale === "en" ? "en-US" : "es");
}

function isPlural(value: unknown): value is Plural {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Record<string, unknown>).other === "string"
  );
}

function lookup(messages: Messages, key: string): string | Plural | undefined {
  let node: unknown = messages;
  for (const part of key.split(".")) {
    if (typeof node !== "object" || node === null) return undefined;
    node = (node as Record<string, unknown>)[part];
  }
  return typeof node === "string" || isPlural(node) ? node : undefined;
}

const PLACEHOLDER = /\{(\w+)\}/g;

/** Formats values for one locale. */
export interface Formatters {
  number(value: number, options?: Intl.NumberFormatOptions): string;
  /** An amount in US dollars (what OpenAI bills in). */
  currency(usd: number, options?: Intl.NumberFormatOptions): string;
  /** A fraction (0.2) as a percentage (20 %). */
  percent(fraction: number): string;
  date(value: Date | number, options?: Intl.DateTimeFormatOptions): string;
}

/** Looks up a message and fills in its parameters. Unknown keys return the key itself. */
export type Translate = (key: MessageKey, params?: MessageParams) => string;

/** Like {@link Translate}, but parameters may be React nodes (for example a `mono` span). */
export type TranslateRich = (
  key: MessageKey,
  params: Record<string, ReactNode>,
) => ReactNode;

export interface Translator {
  locale: Locale;
  t: Translate;
  rich: TranslateRich;
  format: Formatters;
}

/** Builds the translator for `locale`; numbers are formatted with `formatTag`. */
export function createTranslator(
  locale: Locale,
  formatTag: string = locale === "en" ? "en-US" : "es",
): Translator {
  const messages = DICTIONARIES[locale];
  const plurals = new Intl.PluralRules(formatTag);

  const format: Formatters = {
    number: (value, options) =>
      new Intl.NumberFormat(formatTag, options).format(value),
    currency: (usd, options) =>
      new Intl.NumberFormat(formatTag, {
        style: "currency",
        currency: "USD",
        ...options,
      }).format(usd),
    percent: (fraction) =>
      new Intl.NumberFormat(formatTag, {
        style: "percent",
        maximumFractionDigits: 0,
      }).format(fraction),
    date: (value, options) =>
      new Intl.DateTimeFormat(formatTag, options).format(value),
  };

  function template(key: MessageKey, count: unknown): string {
    const entry = lookup(messages, key);
    if (entry === undefined) return key;
    if (typeof entry === "string") return entry;
    const category =
      typeof count === "number" ? plurals.select(count) : "other";
    return entry[category] ?? entry.other;
  }

  const t: Translate = (key, params = {}) =>
    template(key, params.count).replace(PLACEHOLDER, (whole, name: string) => {
      const value = params[name];
      if (value === undefined) return whole;
      return typeof value === "number" ? format.number(value) : value;
    });

  const rich: TranslateRich = (key, params) => {
    const parts = template(key, params.count).split(PLACEHOLDER);
    // `split` with a capture group alternates text and placeholder names.
    return parts.map((part, index) => {
      if (index % 2 === 0) return part;
      const value = params[part];
      return createElement(
        Fragment,
        { key: index },
        value === undefined ? `{${part}}` : value,
      );
    });
  };

  return { locale, t, rich, format };
}

/**
 * The rest of a core error message after its prefix, e.g. `"audio device error: busy"` →
 * `"busy"`. The prefix is replaced by the translated message.
 */
export function errorDetail(message: string): string {
  const index = message.indexOf(": ");
  return index === -1 ? message : message.slice(index + 2);
}
