import { useEffect, useState } from "react";
import Button from "./components/Button";
import Dialog from "./components/Dialog";
import Notice from "./components/Notice";
import { useT } from "./i18n/context";
import { failureText, type Failure } from "./i18n/errors";
import { hideToTray, onCloseToTrayNotice, quitApp } from "./lib/api";

/**
 * The one-off notice the first time the window is closed: LiveDub Voice keeps running in the
 * tray. The core keeps the window open until the user answers (hide or quit); afterwards
 * closing hides the window without asking.
 */
function CloseToTrayDialog() {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Failure | null>(null);

  useEffect(() => {
    let current = true;
    const subscription = onCloseToTrayNotice(() => {
      if (current) setOpen(true);
    });
    // Without the core (a plain browser preview) there is nothing to listen to.
    subscription.catch(() => undefined);
    return () => {
      current = false;
      subscription.then((unlisten) => unlisten()).catch(() => undefined);
    };
  }, []);

  if (!open) return null;

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      setOpen(false);
    } catch (reason: unknown) {
      setError({ key: "closeToTray.error", reason });
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      title={t("closeToTray.title")}
      onDismiss={() => setOpen(false)}
      actions={
        <>
          <Button disabled={busy} onClick={() => void run(hideToTray)}>
            {t("closeToTray.hide")}
          </Button>
          <Button
            variant="secondary"
            disabled={busy}
            onClick={() => void run(quitApp)}
          >
            {t("closeToTray.quit")}
          </Button>
        </>
      }
    >
      <p>{t("closeToTray.text")}</p>
      <p className="ld-dialog__hint">{t("closeToTray.settingHint")}</p>
      {error && <Notice tone="error">{failureText(error, t)}</Notice>}
    </Dialog>
  );
}

export default CloseToTrayDialog;
