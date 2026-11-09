/** Quietest level the meter shows, in dBFS; anything below reads as an empty bar. */
export const METER_FLOOR_DB = -60;

/** Peaks at or above this level (dBFS) are close to clipping. */
export const METER_HOT_DB = -3;

/** Converts a linear amplitude (0–1) to dBFS; silence is `-Infinity`. */
export function amplitudeToDb(amplitude: number): number {
  return amplitude > 0 ? 20 * Math.log10(amplitude) : -Infinity;
}

/**
 * Position of a linear amplitude on the meter, 0–100, on a dB scale from {@link METER_FLOOR_DB}
 * to 0 dBFS (so quiet speech still moves the bar).
 */
export function amplitudeToMeterPercent(amplitude: number): number {
  const db = amplitudeToDb(amplitude);
  if (db <= METER_FLOOR_DB) return 0;
  if (db >= 0) return 100;
  return Math.round(((db - METER_FLOOR_DB) / -METER_FLOOR_DB) * 100);
}
