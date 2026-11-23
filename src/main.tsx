import "@fontsource-variable/plus-jakarta-sans";
import "./styles/tokens.css";
import "./styles/global.css";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import I18nProvider from "./i18n/I18nProvider";
import { loadStartup } from "./lib/onboarding";

// The saved app language and the first-run setup are read before the first render, so the panel
// never flashes English or Home first.
void loadStartup().then(({ uiLanguage, onboarding }) => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <I18nProvider preference={uiLanguage}>
        <App onboarding={onboarding} />
      </I18nProvider>
    </React.StrictMode>,
  );
});
