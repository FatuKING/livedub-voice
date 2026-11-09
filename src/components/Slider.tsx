import type { ReactNode } from "react";
import type { Direction } from "../lib/languages";
import Field from "./Field";
import "./Slider.css";

export interface SliderProps {
  label: ReactNode;
  value: number;
  onChange: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  hint?: ReactNode;
  /** Formats the value shown next to the label. Defaults to the plain number. */
  formatValue?: (value: number) => string;
  /** Accent color. Neutral (ink) when omitted. */
  direction?: Direction;
}

/** Range slider with its current value shown next to the label. */
function Slider({
  label,
  value,
  onChange,
  min = 0,
  max = 100,
  step = 1,
  hint,
  formatValue = String,
  direction,
}: SliderProps) {
  const formatted = formatValue(value);
  return (
    <Field
      label={label}
      hint={hint}
      aside={
        <span className="ld-slider__value mono" aria-hidden="true">
          {formatted}
        </span>
      }
    >
      {(control) => (
        <input
          type="range"
          className={["ld-slider", direction && `ld-slider--${direction}`]
            .filter(Boolean)
            .join(" ")}
          min={min}
          max={max}
          step={step}
          value={value}
          aria-valuetext={formatted}
          onChange={(event) => onChange(Number(event.target.value))}
          {...control}
        />
      )}
    </Field>
  );
}

export default Slider;
