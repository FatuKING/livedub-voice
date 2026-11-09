import { CircleAlert, CircleCheck, Info, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import "./Notice.css";

export type NoticeTone = "info" | "success" | "warning" | "error";

export interface NoticeProps {
  /** `error` is announced as an alert, the rest as a polite status. */
  tone?: NoticeTone;
  children: ReactNode;
}

const ICONS = {
  info: Info,
  success: CircleCheck,
  warning: TriangleAlert,
  error: CircleAlert,
} as const;

/** Inline message: form results, load errors, warnings. */
function Notice({ tone = "info", children }: NoticeProps) {
  const Icon = ICONS[tone];
  return (
    <div
      className={`ld-notice ld-notice--${tone}`}
      role={tone === "error" ? "alert" : "status"}
    >
      <Icon className="ld-notice__icon" size={16} aria-hidden="true" />
      <p>{children}</p>
    </div>
  );
}

export default Notice;
