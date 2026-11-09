import type { ReactNode } from "react";
import type { Direction } from "../lib/languages";
import {
  METER_HOT_DB,
  amplitudeToDb,
  amplitudeToMeterPercent,
} from "../lib/level";
import "./LevelMeter.css";

export interface LevelMeterProps {
  label: ReactNode;
  /** Loudness (RMS), linear 0–1. Drives the bar. */
  level: number;
  /** Peak, linear 0–1. Shown as a tick; turns sun-colored near clipping. */
  peak?: number;
  /** Bar color. Neutral (ink) when omitted. */
  direction?: Direction;
}

/** VU bar on a dB scale (−60 to 0 dBFS). Exposed to assistive tech as a `meter`. */
function LevelMeter({ label, level, peak, direction }: LevelMeterProps) {
  const percent = amplitudeToMeterPercent(level);
  const peakPercent = peak === undefined ? null : amplitudeToMeterPercent(peak);
  const hot = peak !== undefined && amplitudeToDb(peak) >= METER_HOT_DB;
  return (
    <div
      className={["ld-meter", direction && `ld-meter--${direction}`]
        .filter(Boolean)
        .join(" ")}
    >
      <span className="ld-meter__label">{label}</span>
      <div
        className="ld-meter__track"
        role="meter"
        aria-label={typeof label === "string" ? label : undefined}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        aria-valuetext={`${percent}%`}
      >
        <div className="ld-meter__fill" style={{ width: `${percent}%` }} />
        {peakPercent !== null && peakPercent > 0 && (
          <div
            className={["ld-meter__peak", hot && "ld-meter__peak--hot"]
              .filter(Boolean)
              .join(" ")}
            style={{ left: `${peakPercent}%` }}
            aria-hidden="true"
          />
        )}
      </div>
    </div>
  );
}

export default LevelMeter;
