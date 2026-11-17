import { RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import Button from "./components/Button";
import Notice from "./components/Notice";
import SegmentedControl from "./components/SegmentedControl";
import Select from "./components/Select";
import { useT } from "./i18n/context";
import { failureText, type Failure } from "./i18n/errors";
import { listAudioApps, type AudioApp, type SourceApp } from "./lib/api";
import { NO_APP_VALUE, appOptions, appSelectValue } from "./lib/audioApps";
import "./SourceAppPicker.css";

type Mode = "system" | "app";

export interface SourceAppPickerProps {
  value: SourceApp;
  onChange: (value: SourceApp) => void;
}

/**
 * Where the incoming translation listens: the whole system (except LiveDub Voice) or one app,
 * picked from the apps the core reports as playing audio (refreshable).
 */
function SourceAppPicker({ value, onChange }: SourceAppPickerProps) {
  const t = useT();
  const [mode, setMode] = useState<Mode>(
    value.kind === "process" ? "app" : "system",
  );
  const [apps, setApps] = useState<AudioApp[] | null>(null);
  const [error, setError] = useState<Failure | null>(null);
  const [loading, setLoading] = useState(false);

  const fetchApps = useCallback(
    () =>
      listAudioApps().then(
        (list) => {
          setApps(list);
          setError(null);
        },
        (reason: unknown) => {
          setError({ key: "settings.incoming.appsError", reason });
        },
      ),
    [],
  );

  useEffect(() => {
    void fetchApps();
  }, [fetchApps]);

  function refresh() {
    setLoading(true);
    void fetchApps().finally(() => setLoading(false));
  }

  const savedName = value.kind === "process" ? value.name : null;

  // Switching to "one app" (or the list arriving after it) starts with the app most likely
  // wanted: the list puts the ones playing first.
  const firstApp = apps?.[0]?.processName;
  useEffect(() => {
    if (mode === "app" && savedName === null && firstApp !== undefined) {
      onChange({ kind: "process", name: firstApp });
    }
  }, [mode, savedName, firstApp, onChange]);

  function chooseMode(next: Mode) {
    setMode(next);
    if (next === "system") onChange({ kind: "allExceptSelf" });
  }

  return (
    <div className="ld-source">
      <SegmentedControl
        direction="incoming"
        label={t("settings.incoming.source")}
        hint={
          mode === "system"
            ? t("settings.incoming.sourceSystemHint")
            : undefined
        }
        value={mode}
        options={[
          { value: "system", label: t("settings.incoming.sourceSystem") },
          { value: "app", label: t("settings.incoming.sourceApp") },
        ]}
        onChange={chooseMode}
      />
      {mode === "app" && (
        <>
          <Select
            label={t("settings.incoming.sourceAppLabel")}
            hint={t("settings.incoming.sourceAppHint")}
            value={appSelectValue(apps, savedName)}
            options={appOptions({ apps, savedName, t })}
            onChange={(name) => {
              if (name !== NO_APP_VALUE) onChange({ kind: "process", name });
            }}
          />
          <Button
            className="ld-source__refresh"
            variant="ghost"
            disabled={loading}
            icon={<RefreshCw size={16} aria-hidden="true" />}
            onClick={refresh}
          >
            {loading
              ? t("settings.incoming.refreshingApps")
              : t("settings.incoming.refreshApps")}
          </Button>
          {apps?.length === 0 && savedName === null && (
            <Notice>{t("settings.incoming.sourceAppNone")}</Notice>
          )}
          {savedName === null && (
            <Notice tone="warning">
              {t("settings.incoming.sourceAppPending")}
            </Notice>
          )}
          {error && <Notice tone="error">{failureText(error, t)}</Notice>}
        </>
      )}
    </div>
  );
}

export default SourceAppPicker;
