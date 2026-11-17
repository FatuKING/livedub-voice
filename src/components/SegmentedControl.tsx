import { useId, type ReactNode } from "react";
import type { Direction } from "../lib/languages";
import "./SegmentedControl.css";

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
}

export interface SegmentedControlProps<T extends string> {
  label: ReactNode;
  /** Short help text under the control. */
  hint?: ReactNode;
  value: T;
  options: SegmentedOption<T>[];
  onChange: (value: T) => void;
  /** Color of the chosen segment. Neutral (ink) when omitted. */
  direction?: Direction;
}

/**
 * Pick one of a few options shown side by side. Native radio buttons in a fieldset, so arrow keys
 * and screen readers work as for any radio group.
 */
function SegmentedControl<T extends string>({
  label,
  hint,
  value,
  options,
  onChange,
  direction,
}: SegmentedControlProps<T>) {
  const name = useId();
  const hintId = `${name}-hint`;
  return (
    <fieldset
      className={["ld-segmented", direction && `ld-segmented--${direction}`]
        .filter(Boolean)
        .join(" ")}
      aria-describedby={hint ? hintId : undefined}
    >
      <legend className="ld-segmented__legend">{label}</legend>
      <div className="ld-segmented__track">
        {options.map((option) => (
          <label key={option.value} className="ld-segmented__option">
            <input
              type="radio"
              className="ld-segmented__input"
              name={name}
              value={option.value}
              checked={option.value === value}
              onChange={() => onChange(option.value)}
            />
            <span className="ld-segmented__label">{option.label}</span>
          </label>
        ))}
      </div>
      {hint && (
        <p className="ld-segmented__hint" id={hintId}>
          {hint}
        </p>
      )}
    </fieldset>
  );
}

export default SegmentedControl;
