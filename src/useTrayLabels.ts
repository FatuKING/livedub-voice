import { useEffect } from "react";
import { useI18n } from "./i18n/context";
import { setTrayLabels } from "./lib/api";

/**
 * Keeps the tray menu (built by the core) in the panel's language: sends its strings at
 * startup and whenever the app language changes.
 */
export function useTrayLabels(): void {
  const { t, locale } = useI18n();
  useEffect(() => {
    setTrayLabels({
      open: t("tray.open"),
      quit: t("tray.quit"),
      incoming: t("tray.incoming"),
      outgoing: t("tray.outgoing"),
      states: {
        idle: t("tray.states.idle"),
        connecting: t("tray.states.connecting"),
        live: t("tray.states.live"),
        reconnecting: t("tray.states.reconnecting"),
        stopping: t("tray.states.stopping"),
        error: t("tray.states.error"),
      },
    }).catch(() => undefined);
    // `t` changes with the locale.
  }, [t, locale]);
}
