import type { HTMLAttributes, ReactNode } from "react";
import type { Direction } from "../lib/languages";
import "./Card.css";

export interface CardProps extends Omit<HTMLAttributes<HTMLElement>, "title"> {
  /** Adds the direction's color stripe on top. */
  direction?: Direction;
  /** Small uppercase label above the title (for example "Incoming"). */
  eyebrow?: ReactNode;
  title?: ReactNode;
  /** Content aligned to the right of the header (a status pill, for example). */
  aside?: ReactNode;
  /** Element to render as. */
  as?: "section" | "article" | "div";
}

/** Ruta card: a calm surface with a subtle boarding-pass feel. */
function Card({
  direction,
  eyebrow,
  title,
  aside,
  as: Element = "section",
  className,
  children,
  ...rest
}: CardProps) {
  const classes = ["ld-card", direction && `ld-card--${direction}`, className]
    .filter(Boolean)
    .join(" ");
  const hasHeader = eyebrow != null || title != null || aside != null;
  return (
    <Element className={classes} {...rest}>
      {hasHeader && (
        <div className="ld-card__header">
          <div>
            {eyebrow != null && <p className="ld-card__eyebrow">{eyebrow}</p>}
            {title != null && <h2 className="ld-card__title">{title}</h2>}
          </div>
          {aside}
        </div>
      )}
      {children}
    </Element>
  );
}

export default Card;
