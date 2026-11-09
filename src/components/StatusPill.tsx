import type { ReactNode } from "react";
import "./StatusPill.css";

export type StatusTone = "neutral" | "incoming" | "outgoing" | "sun" | "danger";

export interface StatusPillProps {
  /** incoming/outgoing = a direction's state, sun = warning or "coming soon", danger = error. */
  tone?: StatusTone;
  children: ReactNode;
}

/** Small rounded label with a colored dot for states such as "Off", "Live" or "Coming soon". */
function StatusPill({ tone = "neutral", children }: StatusPillProps) {
  return (
    <span className={`ld-pill ld-pill--${tone}`}>
      <span className="ld-pill__dot" aria-hidden="true" />
      {children}
    </span>
  );
}

export default StatusPill;
