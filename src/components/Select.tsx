import { ChevronDown } from "lucide-react";
import type { ReactNode, SelectHTMLAttributes } from "react";
import Field from "./Field";

export interface SelectOption<T extends string> {
  value: T;
  label: string;
}

export interface SelectProps<T extends string> extends Omit<
  SelectHTMLAttributes<HTMLSelectElement>,
  "id" | "value" | "onChange"
> {
  label: ReactNode;
  hint?: ReactNode;
  value: T;
  options: SelectOption<T>[];
  onChange: (value: T) => void;
}

/** Native select styled with Ruta tokens (native keeps keyboard and screen reader support). */
function Select<T extends string>({
  label,
  hint,
  value,
  options,
  onChange,
  ...rest
}: SelectProps<T>) {
  return (
    <Field label={label} hint={hint}>
      {(control) => (
        <div className="ld-select">
          <select
            className="ld-input"
            value={value}
            onChange={(event) => onChange(event.target.value as T)}
            {...control}
            {...rest}
          >
            {options.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
          <ChevronDown
            className="ld-select__chevron"
            size={16}
            aria-hidden="true"
          />
        </div>
      )}
    </Field>
  );
}

export default Select;
