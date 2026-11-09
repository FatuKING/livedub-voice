import "@fontsource-variable/plus-jakarta-sans";
import "./styles/tokens.css";
import "./styles/global.css";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import I18nProvider from "./i18n/I18nProvider";
import type { UiLanguage } from "./i18n/types";
import { getSettings } from "./lib/api";

/** The saved app language, read before the first render so the panel never flashes English. */
async function savedUiLanguage(): Promise<UiLanguage> {
  try {
    return (await getSettings()).uiLanguage;
  } catch {
    // No core (plain Vite preview) or unreadable settings: follow the OS.
    return "system";
  }
}

void savedUiLanguage().then((preference) => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <I18nProvider preference={preference}>
        <App />
      </I18nProvider>
    </React.StrictMode>,
  );
});
