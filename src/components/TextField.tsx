import type { InputHTMLAttributes, ReactNode } from "react";
import Field from "./Field";

export interface TextFieldProps extends Omit<
  InputHTMLAttributes<HTMLInputElement>,
  "id" | "type"
> {
  label: ReactNode;
  hint?: ReactNode;
}

/** Single-line text input with label and optional hint. */
function TextField({ label, hint, className, ...rest }: TextFieldProps) {
  return (
    <Field label={label} hint={hint}>
      {(control) => (
        <input
          type="text"
          className={["ld-input", className].filter(Boolean).join(" ")}
          {...control}
          {...rest}
        />
      )}
    </Field>
  );
}

export default TextField;
