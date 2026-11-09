import { Plane } from "lucide-react";
import type { Language } from "../lib/api";
import { useT } from "../i18n/context";
import { languageName, type Direction } from "../lib/languages";
import "./LanguageRoute.css";

export interface LanguageRouteProps {
  from: Language;
  to: Language;
  /** Colors the route: incoming = teal, outgoing = coral. */
  direction: Direction;
  size?: "sm" | "md";
}

/**
 * The Ruta motif: two language chips joined by a dashed route with a plane in the middle.
 *
 * Announced to screen readers as a single phrase, e.g. "English to Spanish".
 */
function LanguageRoute({
  from,
  to,
  direction,
  size = "md",
}: LanguageRouteProps) {
  const t = useT();
  return (
    <div
      className={`ld-route ld-route--${direction} ld-route--${size}`}
      role="img"
      aria-label={t("languages.route", {
        from: languageName(from, t),
        to: languageName(to, t),
      })}
    >
      <span className="ld-route__chip">{from.toUpperCase()}</span>
      <span className="ld-route__path">
        <Plane className="ld-route__plane" size={size === "sm" ? 14 : 16} />
      </span>
      <span className="ld-route__chip ld-route__chip--to">
        {to.toUpperCase()}
      </span>
    </div>
  );
}

export default LanguageRoute;
