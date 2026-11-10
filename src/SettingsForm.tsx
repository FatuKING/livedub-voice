import { RefreshCw, Save } from "lucide-react";
import { useCallback, useEffect, useState, type FormEvent } from "react";
import Button from "./components/Button";
import Card from "./components/Card";
import LanguageRoute from "./components/LanguageRoute";
import Notice from "./components/Notice";
import Perforation from "./components/Perforation";
import Select from "./components/Select";
import Slider from "./components/Slider";
import TextField from "./components/TextField";
import Toggle from "./components/Toggle";
import { useI18n } from "./i18n/context";
import { failureText, type Failure } from "./i18n/errors";
import {
  getSettings,
  listAudioDevices,
  saveSettings,
  type AudioDevice,
  type Settings,
} from "./lib/api";
import MicTest from "./MicTest";
import {
  SYSTEM_DEFAULT_VALUE,
  deviceIdFromValue,
  deviceOptions,
  hasVirtualCable,
} from "./lib/devices";
import { languageOptions } from "./lib/languages";
import "./SettingsForm.css";

/**
 * Settings form: languages, ducking, enabled toggles, devices and source app, grouped per direction.
 *
 * Devices are picked from the list the core reports (refreshable); the source app is a plain text
 * field until process listing exists.
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

  const { incoming, outgoing } = settings;

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

  const sourceProcess =
    incoming.sourceApp.kind === "process" ? incoming.sourceApp.name : "";

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
          <TextField
            label={t("settings.incoming.sourceApp")}
            hint={t("settings.incoming.sourceAppHint")}
            placeholder={t("settings.incoming.sourceAppPlaceholder")}
            value={sourceProcess}
            onChange={(event) => {
              const name = event.target.value;
              updateIncoming({
                sourceApp:
                  name === ""
                    ? { kind: "allExceptSelf" }
                    : { kind: "process", name },
              });
            }}
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
