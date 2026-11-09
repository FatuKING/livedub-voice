import type { UiLanguage } from "../lib/api";
import type { en } from "./en";

/** Panel languages. */
export type Locale = Exclude<UiLanguage, "system">;

export type { UiLanguage };

/** A plural message: one form per CLDR plural category, `other` is required. */
export interface Plural {
  zero?: string;
  one?: string;
  two?: string;
  few?: string;
  many?: string;
  other: string;
}

type Leaf = string | Plural;

/** The shape of `en` with every string widened, so other languages can fill it in. */
type Widen<T> = T extends string
  ? string
  : T extends Plural
    ? { -readonly [K in keyof T]: string }
    : { -readonly [K in keyof T]: Widen<T[K]> };

/** A full dictionary. A missing (or extra) key fails the type check. */
export type Messages = Widen<typeof en>;

type KeysOf<T, Prefix extends string = ""> = {
  [K in keyof T & string]: T[K] extends Leaf
    ? `${Prefix}${K}`
    : KeysOf<T[K], `${Prefix}${K}.`>;
}[keyof T & string];

/** Dotted path of a message, e.g. `"settings.title"`. */
export type MessageKey = KeysOf<Messages>;

/** Values for `{name}` placeholders. Numbers are formatted for the locale. */
export type MessageParams = Record<string, string | number>;
