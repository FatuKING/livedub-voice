import type { ReactNode } from "react";

export interface PageHeaderProps {
  title: string;
  description?: ReactNode;
}

/**
 * Screen title. The heading is focusable (`tabIndex={-1}`) so the shell can move focus to it
 * after navigation, and screen readers announce the new screen.
 */
function PageHeader({ title, description }: PageHeaderProps) {
  return (
    <header className="ld-page__header">
      <h1 className="ld-page__title" tabIndex={-1}>
        {title}
      </h1>
      {description && <p className="ld-page__description">{description}</p>}
    </header>
  );
}

export default PageHeader;
