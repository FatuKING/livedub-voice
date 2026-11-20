import type { Formatters } from "../i18n/translate";

/** OpenAI's budget settings: the real safety net behind the in-app spending cap. */
export const OPENAI_LIMITS_URL =
  "https://platform.openai.com/settings/organization/limits";

/** Highest spending cap the core accepts (`MAX_SPENDING_CAP_USD` in the settings). */
export const MAX_SPENDING_CAP_USD = 10_000;

/** Seconds of audio as minutes, with one decimal (`12.5`). */
export function formatMinutes(format: Formatters, seconds: number): string {
  return format.number(seconds / 60, {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  });
}

/** An estimated cost: cents, plus a tenth of a cent under a dollar (`$0.034`, `$12.50`). */
export function formatCost(format: Formatters, usd: number): string {
  return format.currency(usd, {
    minimumFractionDigits: 2,
    maximumFractionDigits: usd > 0 && usd < 1 ? 3 : 2,
  });
}

/**
 * Parses a spending cap typed by the user: empty = no cap (`null`), a positive amount up to
 * {@link MAX_SPENDING_CAP_USD} (a decimal comma is accepted), or `undefined` when invalid.
 */
export function parseCap(text: string): number | null | undefined {
  const trimmed = text.trim().replace(/^\$/, "").trim();
  if (trimmed === "") return null;
  if (!/^\d+([.,]\d+)?$/.test(trimmed)) return undefined;
  const value = Number(trimmed.replace(",", "."));
  if (!Number.isFinite(value) || value <= 0 || value > MAX_SPENDING_CAP_USD) {
    return undefined;
  }
  return value;
}

/** A cap as the text of its field (`""` for no cap). */
export function capText(usd: number | null): string {
  return usd === null ? "" : String(usd);
}

/** A local calendar day (`2026-11-20`) as a `Date` at local midnight. */
export function localDay(date: string): Date {
  const [year, month, day] = date.split("-").map(Number);
  return new Date(year, month - 1, day);
}
