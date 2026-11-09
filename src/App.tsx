import { ChartColumn, House, Settings, type LucideIcon } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import "./App.css";
import LogoMark from "./components/LogoMark";
import { getAppInfo, isAppError, type AppInfo } from "./lib/api";
import HomeScreen from "./screens/HomeScreen";
import SettingsScreen from "./screens/SettingsScreen";
import UsageScreen from "./screens/UsageScreen";

export type Screen = "home" | "settings" | "usage";

const NAV_ITEMS: { screen: Screen; label: string; icon: LucideIcon }[] = [
  { screen: "home", label: "Home", icon: House },
  { screen: "settings", label: "Settings", icon: Settings },
  { screen: "usage", label: "Usage", icon: ChartColumn },
];

function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [screen, setScreen] = useState<Screen>("home");
  const contentRef = useRef<HTMLElement>(null);
  const navigated = useRef(false);

  useEffect(() => {
    getAppInfo()
      .then(setInfo)
      .catch((reason: unknown) => {
        setError(isAppError(reason) ? reason.message : String(reason));
      });
  }, []);

  // After the user navigates, move focus to the new screen's title so it is announced.
  useEffect(() => {
    if (!navigated.current) return;
    contentRef.current?.querySelector<HTMLElement>("h1")?.focus();
  }, [screen]);

  function navigate(next: Screen) {
    navigated.current = true;
    setScreen(next);
  }

  return (
    <div className="ld-shell">
      <aside className="ld-sidebar">
        <div className="ld-brand">
          <LogoMark />
          <span className="ld-brand__name">LiveDub Voice</span>
        </div>

        <nav className="ld-nav" aria-label="Main">
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
                  {label}
                </button>
              </li>
            ))}
          </ul>
        </nav>

        <footer className="ld-sidebar__footer">
          {info && <p className="mono">Version {info.version}</p>}
          {error && <p role="alert">Could not read app info: {error}</p>}
        </footer>
      </aside>

      <main className="ld-content" ref={contentRef}>
        <div className="ld-content__inner">
          {screen === "home" && (
            <HomeScreen onOpenSettings={() => navigate("settings")} />
          )}
          {screen === "settings" && <SettingsScreen />}
          {screen === "usage" && <UsageScreen />}
        </div>
      </main>
    </div>
  );
}

export default App;
