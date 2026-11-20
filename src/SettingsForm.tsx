import { LogOut, RefreshCw, Save } from "lucide-react";
import { useCallback, useEffect, useState, type FormEvent } from "react";
import Button from "./components/Button";
import Card from "./components/Card";
import LanguageRoute from "./components/LanguageRoute";
import Notice from "./components/Notice";
import Perforation from "./components/Perforation";
import Select from "./components/Select";
import Slider from "./components/Slider";
import Toggle from "./components/Toggle";
import { useI18n } from "./i18n/context";
import { failureText, type Failure } from "./i18n/errors";
import {
  getSettings,
  listAudioDevices,
  quitApp,
  saveSettings,
  type AudioDevice,
  type Settings,
} from "./lib/api";
import MicTest from "./MicTest";
import SourceAppPicker from "./SourceAppPicker";
import {
  SYSTEM_DEFAULT_VALUE,
  deviceIdFromValue,
  deviceOptions,
  hasVirtualCable,
} from "./lib/devices";
import { languageOptions, type Direction } from "./lib/languages";
import { PIPELINES, isActive } from "./lib/pipelines";
import "./SettingsForm.css";

/** Auto-off times offered (minutes); a saved value outside the list is added to it. */
const AUTO_OFF_CHOICES = [1, 2, 5, 10, 15, 30, 60];

/** The auto-off select's value for "never". */
const NEVER = "never";

/**
 * Settings form: languages, ducking, enabled toggles, devices and source app, grouped per
 * direction; then voice detection and what closing the window does, for the whole app.
 *
 * Devices are picked from the list the core reports (refreshable); the incoming source is the
 * whole system or one of the apps playing audio (see `SourceAppPicker`).
 */
function SettingsForm() {
  const { t, format, preference, locale } = useI18n();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<Failure | null>(null);
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);
  const [devices, setDevices] = useState<AudioDevice[] | null>(null);
  const [devicesError, setDevicesError] = useState<Failure | null>(null);
  const [loadingDevices, setLoadingDevices] = useState(false);
  const [quitError, setQuitError] = useState<Failure | null>(null);
  const [active, setActive] = useState<Record<Direction, boolean>>({
    incoming: false,
    outgoing: false,
  });

  const fetchDevices = useCallback(
    () =>
      listAudioDevices().then(
        (list) => {
          setDevices(list);
          setDevicesError(null);
        },
        (reason: unknown) => {
          setDevicesError({ key: "settings.devicesError", reason });
        },
      ),
    [],
  );

  useEffect(() => {
    void fetchDevices();
  }, [fetchDevices]);

  function refreshDevices() {
    setLoadingDevices(true);
    void fetchDevices().finally(() => setLoadingDevices(false));
  }

  // A running direction keeps the settings it started with (see the note in each card).
  useEffect(() => {
    let current = true;
    for (const direction of ["incoming", "outgoing"] as const) {
      Promise.resolve()
        .then(() => PIPELINES[direction].getStatus())
        .then((status) => {
          if (current)
            setActive((value) => ({
              ...value,
              [direction]: isActive(status.state),
            }));
        })
        .catch(() => undefined);
    }
    return () => {
      current = false;
    };
  }, []);

  useEffect(() => {
    getSettings()
      .then(setSettings)
      .catch((reason: unknown) => {
        setError({ key: "settings.loadError", reason });
      });
  }, []);

  if (!settings) {
    return error ? (
      <Notice tone="error">{failureText(error, t)}</Notice>
    ) : (
      <p className="ld-settings__loading">{t("settings.loading")}</p>
    );
  }

  const { incoming, outgoing, voiceActivity } = settings;

  function updateIncoming(patch: Partial<Settings["incoming"]>) {
    setSaved(false);
    setSettings((current) =>
      current
        ? { ...current, incoming: { ...current.incoming, ...patch } }
        : current,
    );
  }

  function updateOutgoing(patch: Partial<Settings["outgoing"]>) {
    setSaved(false);
    setSettings((current) =>
      current
        ? { ...current, outgoing: { ...current.outgoing, ...patch } }
        : current,
    );
  }

  function updateVoice(patch: Partial<Settings["voiceActivity"]>) {
    setSaved(false);
    setSettings((current) =>
      current
        ? {
            ...current,
            voiceActivity: { ...current.voiceActivity, ...patch },
          }
        : current,
    );
  }

  function updateCloseQuitsApp(closeQuitsApp: boolean) {
    setSaved(false);
    setSettings((current) =>
      current ? { ...current, closeQuitsApp } : current,
    );
  }

  function quit() {
    setQuitError(null);
    quitApp().catch((reason: unknown) =>
      setQuitError({ key: "settings.window.quitError", reason }),
    );
  }

  const autoOffChoices = [...AUTO_OFF_CHOICES];
  const savedAutoOff = voiceActivity.autoOffMinutes;
  if (savedAutoOff !== null && !autoOffChoices.includes(savedAutoOff)) {
    autoOffChoices.push(savedAutoOff);
    autoOffChoices.sort((a, b) => a - b);
  }
  const autoOffOptions = [
    { value: NEVER, label: t("settings.voice.autoOffNever") },
    ...autoOffChoices.map((minutes) => ({
      value: String(minutes),
      label: t("settings.voice.autoOffMinutes", { count: minutes }),
    })),
  ];

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!settings) return;
    setSaving(true);
    setError(null);
    setSaved(false);
    try {
      // The app language is saved on its own as soon as it changes: keep the current one.
      setSettings(await saveSettings({ ...settings, uiLanguage: preference }));
      setSaved(true);
    } catch (reason: unknown) {
      setError({ key: "settings.saveError", reason });
    } finally {
      setSaving(false);
    }
  }

  const missingCable = devices !== null && !hasVirtualCable(devices, "output");

  return (
    <form
      className="ld-settings"
      onSubmit={(event) => void handleSubmit(event)}
    >
      <div className="ld-route-grid">
        <Card
          direction="incoming"
          eyebrow={t("settings.incoming.eyebrow")}
          title={t("settings.incoming.title")}
          aria-label={t("settings.incoming.ariaLabel")}
          aside={
            <LanguageRoute
              direction="incoming"
              size="sm"
              to={incoming.targetLanguage}
            />
          }
        >
          {active.incoming && (
            <Notice tone="info">
              {t("settings.incoming.appliesNextStart")}
            </Notice>
          )}
          <Toggle
            direction="incoming"
            label={t("settings.incoming.enabled")}
            checked={incoming.enabled}
            onChange={(enabled) => updateIncoming({ enabled })}
          />
          <Select
            label={t("settings.incoming.targetLanguage")}
            value={incoming.targetLanguage}
            options={languageOptions(locale)}
            onChange={(targetLanguage) => updateIncoming({ targetLanguage })}
          />
          <SourceAppPicker
            value={incoming.sourceApp}
            onChange={(sourceApp) => updateIncoming({ sourceApp })}
          />
          <Select
            label={t("settings.incoming.headphones")}
            hint={t("settings.incoming.headphonesHint")}
            value={incoming.outputDeviceId ?? SYSTEM_DEFAULT_VALUE}
            options={deviceOptions({
              devices,
              kind: "output",
              savedId: incoming.outputDeviceId,
              t,
            })}
            onChange={(value) =>
              updateIncoming({ outputDeviceId: deviceIdFromValue(value) })
            }
          />
          <Slider
            direction="incoming"
            label={t("settings.incoming.originalVolume")}
            hint={t("settings.incoming.originalVolumeHint")}
            value={incoming.originalVolume}
            formatValue={(value) => format.percent(value / 100)}
            onChange={(originalVolume) => updateIncoming({ originalVolume })}
          />
        </Card>

        <Card
          direction="outgoing"
          eyebrow={t("settings.outgoing.eyebrow")}
          title={t("settings.outgoing.title")}
          aria-label={t("settings.outgoing.ariaLabel")}
          aside={
            <LanguageRoute
              direction="outgoing"
              size="sm"
              to={outgoing.targetLanguage}
            />
          }
        >
          {active.outgoing && (
            <Notice tone="info">
              {t("settings.outgoing.appliesNextStart")}
            </Notice>
          )}
          <Toggle
            direction="outgoing"
            label={t("settings.outgoing.enabled")}
            checked={outgoing.enabled}
            onChange={(enabled) => updateOutgoing({ enabled })}
          />
          <Select
            label={t("settings.outgoing.targetLanguage")}
            value={outgoing.targetLanguage}
            options={languageOptions(locale)}
            onChange={(targetLanguage) => updateOutgoing({ targetLanguage })}
          />
          <Select
            label={t("settings.outgoing.microphone")}
            value={outgoing.inputDeviceId ?? SYSTEM_DEFAULT_VALUE}
            options={deviceOptions({
              devices,
              kind: "input",
              savedId: outgoing.inputDeviceId,
              t,
            })}
            onChange={(value) =>
              updateOutgoing({ inputDeviceId: deviceIdFromValue(value) })
            }
          />
          <Select
            label={t("settings.outgoing.virtualCable")}
            hint={
              missingCable
                ? t("settings.outgoing.virtualCableMissing")
                : t("settings.outgoing.virtualCableHint")
            }
            value={outgoing.outputDeviceId ?? SYSTEM_DEFAULT_VALUE}
            options={deviceOptions({
              devices,
              kind: "output",
              savedId: outgoing.outputDeviceId,
              cablesFirst: true,
              t,
            })}
            onChange={(value) =>
              updateOutgoing({ outputDeviceId: deviceIdFromValue(value) })
            }
          />
          <Perforation />
          <MicTest
            inputId={outgoing.inputDeviceId}
            outputId={incoming.outputDeviceId}
          />
        </Card>
      </div>

      <div className="ld-route-grid">
        <Card
          eyebrow={t("settings.voice.eyebrow")}
          title={t("settings.voice.title")}
          aria-label={t("settings.voice.ariaLabel")}
        >
          {(active.incoming || active.outgoing) && (
            <Notice tone="info">{t("settings.voice.appliesNextStart")}</Notice>
          )}
          <Toggle
            label={t("settings.voice.pauseWhenSilent")}
            hint={t("settings.voice.pauseWhenSilentHint")}
            checked={voiceActivity.pauseWhenSilent}
            onChange={(pauseWhenSilent) => updateVoice({ pauseWhenSilent })}
          />
          <Select
            label={t("settings.voice.autoOff")}
            hint={t("settings.voice.autoOffHint")}
            value={savedAutoOff === null ? NEVER : String(savedAutoOff)}
            options={autoOffOptions}
            onChange={(value) =>
              updateVoice({
                autoOffMinutes: value === NEVER ? null : Number(value),
              })
            }
          />
        </Card>

        <Card
          eyebrow={t("settings.window.eyebrow")}
          title={t("settings.window.title")}
          aria-label={t("settings.window.ariaLabel")}
        >
          <Toggle
            label={t("settings.window.closeQuitsApp")}
            hint={t("settings.window.closeQuitsAppHint")}
            checked={settings.closeQuitsApp}
            onChange={updateCloseQuitsApp}
          />
          <Perforation />
          <div className="ld-settings__quit">
            <Button
              variant="secondary"
              icon={<LogOut size={16} aria-hidden="true" />}
              onClick={quit}
            >
              {t("settings.window.quit")}
            </Button>
            <p className="ld-settings__hint">{t("settings.window.quitHint")}</p>
            {quitError && (
              <Notice tone="error">{failureText(quitError, t)}</Notice>
            )}
          </div>
        </Card>
      </div>

      <div className="ld-settings__footer">
        <Button
          type="submit"
          disabled={saving}
          icon={<Save size={16} aria-hidden="true" />}
        >
          {saving ? t("settings.saving") : t("settings.save")}
        </Button>
        <Button
          variant="secondary"
          disabled={loadingDevices}
          icon={<RefreshCw size={16} aria-hidden="true" />}
          onClick={refreshDevices}
        >
          {loadingDevices
            ? t("settings.refreshing")
            : t("settings.refreshDevices")}
        </Button>
        {devicesError && (
          <Notice tone="error">{failureText(devicesError, t)}</Notice>
        )}
        {saved && <Notice tone="success">{t("settings.saved")}</Notice>}
        {error && <Notice tone="error">{failureText(error, t)}</Notice>}
      </div>
    </form>
  );
}

export default SettingsForm;
