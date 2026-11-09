import { useId, type ReactNode } from "react";
import type { Direction } from "../lib/languages";
import "./Toggle.css";

export interface ToggleProps {
  label: ReactNode;
  checked: boolean;
  onChange: (checked: boolean) => void;
  hint?: ReactNode;
  /** Color of the "on" state. Neutral (ink) when omitted. */
  direction?: Direction;
  disabled?: boolean;
}

/** On/off switch. A native checkbox with `role="switch"`, so it stays keyboard and form friendly. */
function Toggle({
  label,
  checked,
  onChange,
  hint,
  direction,
  disabled,
}: ToggleProps) {
  const hintId = useId();
  return (
    <div
      className={["ld-toggle", direction && `ld-toggle--${direction}`]
        .filter(Boolean)
        .join(" ")}
    >
      <label className="ld-toggle__label">
        <input
          type="checkbox"
          role="switch"
          className="ld-toggle__input"
          checked={checked}
          disabled={disabled}
          aria-describedby={hint ? hintId : undefined}
          onChange={(event) => onChange(event.target.checked)}
        />
        <span className="ld-toggle__track" aria-hidden="true">
          <span className="ld-toggle__thumb" />
        </span>
        <span className="ld-toggle__text">{label}</span>
      </label>
      {hint && (
        <p className="ld-toggle__hint" id={hintId}>
          {hint}
        </p>
      )}
    </div>
  );
}

export default Toggle;
