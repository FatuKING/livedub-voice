import { ChartColumn, House, Settings, type LucideIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import "./App.css";
import CloseToTrayDialog from "./CloseToTrayDialog";
import LogoMark from "./components/LogoMark";
import { useT } from "./i18n/context";
import { failureText, type Failure } from "./i18n/errors";
import type { MessageKey } from "./i18n/types";
import { getAppInfo, type AppInfo } from "./lib/api";
import type { OnboardingMode } from "./lib/onboarding";
import Onboarding from "./onboarding/Onboarding";
import HomeScreen from "./screens/HomeScreen";
import SettingsScreen from "./screens/SettingsScreen";
import UsageScreen from "./screens/UsageScreen";
import { useTrayLabels } from "./useTrayLabels";

export type Screen = "home" | "settings" | "usage";

const NAV_ITEMS: { screen: Screen; label: MessageKey; icon: LucideIcon }[] = [
  { screen: "home", label: "app.nav.home", icon: House },
  { screen: "settings", label: "app.nav.settings", icon: Settings },
  { screen: "usage", label: "app.nav.usage", icon: ChartColumn },
];

export interface AppProps {
  /** The first-run setup to show at launch (see `loadStartup`); none by default. */
  onboarding?: OnboardingMode | null;
}

function App({ onboarding: initialOnboarding = null }: AppProps) {
  const t = useT();
  const [onboarding, setOnboarding] = useState<OnboardingMode | null>(
    initialOnboarding,
  );
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<Failure | null>(null);
  const [screen, setScreen] = useState<Screen>("home");
  const contentRef = useRef<HTMLElement>(null);
  const navigated = useRef(false);
  useTrayLabels();

  useEffect(() => {
    getAppInfo()
      .then(setInfo)
      .catch((reason: unknown) => setError({ key: "app.infoError", reason }));
  }, []);

  // After the user navigates, move focus to the new screen's title so it is announced.
  useEffect(() => {
    if (!navigated.current || onboarding) return;
    contentRef.current?.querySelector<HTMLElement>("h1")?.focus();
  }, [screen, onboarding]);

  function navigate(next: Screen) {
    navigated.current = true;
    setScreen(next);
  }

  /** The setup is over: land on Home. */
  function finishOnboarding() {
    navigated.current = true;
    setScreen("home");
    setOnboarding(null);
  }

  if (onboarding) {
    return (
      <>
        <Onboarding mode={onboarding} onDone={finishOnboarding} />
        <CloseToTrayDialog />
      </>
    );
  }

  return (
    <div className="ld-shell">
      <aside className="ld-sidebar">
        <div className="ld-brand">
          <LogoMark />
          <span className="ld-brand__name">LiveDub Voice</span>
        </div>

        <nav className="ld-nav" aria-label={t("app.navLabel")}>
          <ul>
            {NAV_ITEMS.map(({ screen: target, label, icon: Icon }) => (
              <li key={target}>
                <button
                  type="button"
                  className="ld-nav__item"
                  aria-current={screen === target ? "page" : undefined}
                  onClick={() => navigate(target)}
                >
                  <Icon size={18} aria-hidden="true" />
                  {t(label)}
                </button>
              </li>
            ))}
          </ul>
        </nav>

        <footer className="ld-sidebar__footer">
          {info && (
            <p className="mono">
              {t("app.version", { version: info.version })}
            </p>
          )}
          {error && <p role="alert">{failureText(error, t)}</p>}
        </footer>
      </aside>

      <main className="ld-content" ref={contentRef}>
        <div className="ld-content__inner">
          {screen === "home" && (
            <HomeScreen
              onOpenSettings={() => navigate("settings")}
              onOpenUsage={() => navigate("usage")}
            />
          )}
          {screen === "settings" && (
            <SettingsScreen onRunSetup={() => setOnboarding("full")} />
          )}
          {screen === "usage" && <UsageScreen />}
        </div>
      </main>
      <CloseToTrayDialog />
    </div>
  );
}

export default App;
