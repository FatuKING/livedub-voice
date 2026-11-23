import { Check } from "lucide-react";
import "./Stepper.css";

export interface StepperStep {
  id: string;
  label: string;
}

export interface StepperProps {
  /** Accessible name of the list (for example "Setup steps"). */
  label: string;
  steps: StepperStep[];
  /** Index of the current step. */
  current: number;
  /** Jumps to a step. */
  onSelect: (index: number) => void;
  /** Screen-reader suffix for steps before the current one (for example "done"). */
  doneLabel: string;
}

/**
 * Vertical step indicator: numbered steps, the current one marked like the active nav item
 * (amber marker, `aria-current="step"`). Every step is a button, so any step can be reached by
 * keyboard.
 */
function Stepper({ label, steps, current, onSelect, doneLabel }: StepperProps) {
  return (
    <nav className="ld-stepper" aria-label={label}>
      <ol>
        {steps.map((step, index) => {
          const done = index < current;
          return (
            <li key={step.id}>
              <button
                type="button"
                className={[
                  "ld-stepper__item",
                  done && "ld-stepper__item--done",
                ]
                  .filter(Boolean)
                  .join(" ")}
                aria-current={index === current ? "step" : undefined}
                // Starts with the visible label, so voice control still finds it.
                aria-label={done ? `${step.label} (${doneLabel})` : undefined}
                onClick={() => onSelect(index)}
              >
                <span className="ld-stepper__marker" aria-hidden="true">
                  {done ? <Check size={14} /> : index + 1}
                </span>
                <span>{step.label}</span>
              </button>
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

export default Stepper;
