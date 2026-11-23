import type { ReactNode } from "react";

export interface PageHeaderProps {
  title: string;
  description?: ReactNode;
  /** Small uppercase label above the title (for example "Step 2 of 6"). */
  eyebrow?: ReactNode;
  /** Buttons aligned to the right of the title (for example "How to use"). */
  actions?: ReactNode;
}

/**
 * Screen title. The heading is focusable (`tabIndex={-1}`) so the shell can move focus to it
 * after navigation, and screen readers announce the new screen.
 */
function PageHeader({ title, description, eyebrow, actions }: PageHeaderProps) {
  return (
    <header className="ld-page__header">
      <div className="ld-page__heading">
        <div className="ld-page__titles">
          {eyebrow != null && <p className="ld-page__eyebrow">{eyebrow}</p>}
          <h1 className="ld-page__title" tabIndex={-1}>
            {title}
          </h1>
        </div>
        {actions != null && <div className="ld-page__actions">{actions}</div>}
      </div>
      {description && <p className="ld-page__description">{description}</p>}
    </header>
  );
}

export default PageHeader;
