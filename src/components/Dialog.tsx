import { useEffect, useId, useRef, type ReactNode } from "react";
import "./Dialog.css";

export interface DialogProps {
  title: string;
  children: ReactNode;
  /** The buttons, at the bottom. The first one gets the focus when the dialog opens. */
  actions: ReactNode;
  /** Escape closes the dialog through this. */
  onDismiss: () => void;
}

/**
 * Modal dialog over the whole panel, for the rare question that must be answered before going
 * on. Focus moves to its first action and stays inside it; Escape dismisses it.
 */
function Dialog({ title, children, actions, onDismiss }: DialogProps) {
  const titleId = useId();
  const panel = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    panel.current
      ?.querySelector<HTMLElement>(".ld-dialog__actions button")
      ?.focus();
    return () => previous?.focus();
  }, []);

  function onKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.stopPropagation();
      onDismiss();
      return;
    }
    if (event.key !== "Tab" || !panel.current) return;
    // Keep the focus inside the dialog.
    const focusable = Array.from(
      panel.current.querySelectorAll<HTMLElement>(
        "button:not([disabled]), [href], input, select, textarea",
      ),
    );
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (!first || !last) return;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  return (
    <div className="ld-dialog-backdrop">
      <div
        ref={panel}
        className="ld-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onKeyDown={onKeyDown}
      >
        <h2 className="ld-dialog__title" id={titleId}>
          {title}
        </h2>
        <div className="ld-dialog__body">{children}</div>
        <div className="ld-dialog__actions">{actions}</div>
      </div>
    </div>
  );
}

export default Dialog;
