import { AudioLines, Ear } from "lucide-react";
import type { Direction } from "../lib/languages";
import "./VoiceIndicator.css";

export interface VoiceIndicatorProps {
  /** Colors "speaking" in the direction's color. */
  direction: Direction;
  speaking: boolean;
  /** What is shown: "Speaking" or "Listening". */
  label: string;
  /** Name of the indicator for screen readers, e.g. "Voice". */
  name: string;
  /** Why, in muted text after the label (e.g. "no audio is sent"). */
  hint?: string;
}

/**
 * Whether a running direction hears someone speaking: an icon and a word, in the direction's
 * color while speaking, muted while listening. Text, not only color, carries the state.
 */
function VoiceIndicator({
  direction,
  speaking,
  label,
  name,
  hint,
}: VoiceIndicatorProps) {
  const Icon = speaking ? AudioLines : Ear;
  return (
    <p
      className={[
        "ld-voice",
        `ld-voice--${direction}`,
        speaking && "ld-voice--speaking",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      <Icon className="ld-voice__icon" size={16} aria-hidden="true" />
      <span className="ld-voice__label">
        <span className="visually-hidden">{name}: </span>
        {label}
      </span>
      {hint && <span className="ld-voice__hint">{hint}</span>}
    </p>
  );
}

export default VoiceIndicator;
