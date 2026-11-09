import type { ButtonHTMLAttributes, ReactNode } from "react";
import "./Button.css";

export type ButtonVariant =
  "primary" | "secondary" | "ghost" | "incoming" | "outgoing";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** `incoming` / `outgoing` are reserved for actions that belong to a translation direction. */
  variant?: ButtonVariant;
  /** Optional icon shown before the label. */
  icon?: ReactNode;
}

/** Ruta button. Defaults to `type="button"` so it never submits a form by accident. */
function Button({
  variant = "primary",
  icon,
  className,
  children,
  type = "button",
  ...rest
}: ButtonProps) {
  const classes = ["ld-button", `ld-button--${variant}`, className]
    .filter(Boolean)
    .join(" ");
  return (
    <button type={type} className={classes} {...rest}>
      {icon}
      {children}
    </button>
  );
}

export default Button;
