import { useId, type ReactNode } from "react";
import "./Field.css";

/** Props a Field hands to its control so the label and hint are wired up. */
export interface FieldControlProps {
  id: string;
  "aria-describedby"?: string;
}

export interface FieldProps {
  label: ReactNode;
  /** Short help text under the control. */
  hint?: ReactNode;
  /** Content aligned to the right of the label (for example a slider value). */
  aside?: ReactNode;
  /** Renders the control with the ids it needs. */
  children: (control: FieldControlProps) => ReactNode;
}

/** Label + control + hint. Building block for TextField, Select and Slider. */
function Field({ label, hint, aside, children }: FieldProps) {
  const id = useId();
  const hintId = `${id}-hint`;
  return (
    <div className="ld-field">
      <div className="ld-field__label-row">
        <label className="ld-field__label" htmlFor={id}>
          {label}
        </label>
        {aside}
      </div>
      {children({ id, "aria-describedby": hint ? hintId : undefined })}
      {hint && (
        <p className="ld-field__hint" id={hintId}>
          {hint}
        </p>
      )}
    </div>
  );
}

export default Field;
