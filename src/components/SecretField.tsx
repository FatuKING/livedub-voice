import { Eye, EyeOff } from "lucide-react";
import { useState, type InputHTMLAttributes, type ReactNode } from "react";
import { useT } from "../i18n/context";
import Field from "./Field";
import "./SecretField.css";

export interface SecretFieldProps extends Omit<
  InputHTMLAttributes<HTMLInputElement>,
  "id" | "type"
> {
  label: ReactNode;
  hint?: ReactNode;
}

/** Password-style input with a show/hide toggle, for secrets such as an API key. */
function SecretField({ label, hint, className, ...rest }: SecretFieldProps) {
  const t = useT();
  const [visible, setVisible] = useState(false);
  return (
    <Field label={label} hint={hint}>
      {(control) => (
        <div className="ld-secret">
          <input
            type={visible ? "text" : "password"}
            className={["ld-input", "ld-secret__input", className]
              .filter(Boolean)
              .join(" ")}
            autoComplete="off"
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
            {...control}
            {...rest}
          />
          <button
            type="button"
            className="ld-secret__toggle"
            aria-label={
              visible
                ? t("components.secretField.hide")
                : t("components.secretField.show")
            }
            aria-pressed={visible}
            onClick={() => setVisible((current) => !current)}
          >
            {visible ? (
              <EyeOff size={16} aria-hidden="true" />
            ) : (
              <Eye size={16} aria-hidden="true" />
            )}
          </button>
        </div>
      )}
    </Field>
  );
}

export default SecretField;
