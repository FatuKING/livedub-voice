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
import {
  getSettings,
  isAppError,
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
import { LANGUAGE_OPTIONS, sourceLanguageFor } from "./lib/languages";
import "./SettingsForm.css";

function errorMessage(reason: unknown): string {
  return isAppError(reason) ? reason.message : String(reason);
}

/**
 * Settings form: languages, ducking, enabled toggles, devices and source app, grouped per direction.
 *
 * Devices are picked from the list the core reports (refreshable); the source app is a plain text
 * field until process listing exists.
 */
function SettingsForm() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [devices, setDevices] = useState<AudioDevice[] | null>(null);
  const [devicesError, setDevicesError] = useState<string | null>(null);
  const [loadingDevices, setLoadingDevices] = useState(false);

  const fetchDevices = useCallback(
    () =>
      listAudioDevices().then(
        (list) => {
          setDevices(list);
          setDevicesError(null);
        },
        (reason: unknown) => {
          setDevicesError(
            `Could not list audio devices: ${errorMessage(reason)}`,
          );
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
        setError(`Could not load settings: ${errorMessage(reason)}`);
      });
  }, []);

  if (!settings) {
    return error ? (
      <Notice tone="error">{error}</Notice>
    ) : (
      <p className="ld-settings__loading">Loading settings…</p>
    );
  }

  const { incoming, outgoing } = settings;

  function updateIncoming(patch: Partial<Settings["incoming"]>) {
    setStatus(null);
    setSettings((current) =>
      current
        ? { ...current, incoming: { ...current.incoming, ...patch } }
        : current,
    );
  }

  function updateOutgoing(patch: Partial<Settings["outgoing"]>) {
    setStatus(null);
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
    setStatus(null);
    try {
      setSettings(await saveSettings(settings));
      setStatus("Settings saved.");
    } catch (reason: unknown) {
      setError(`Could not save settings: ${errorMessage(reason)}`);
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
          eyebrow="Incoming"
          title="Others → you"
          aria-label="Incoming (others → you)"
          aside={
            <LanguageRoute
              direction="incoming"
              size="sm"
              from={sourceLanguageFor(incoming.targetLanguage)}
              to={incoming.targetLanguage}
            />
          }
        >
          <Toggle
            direction="incoming"
            label="Translate incoming audio"
            checked={incoming.enabled}
            onChange={(enabled) => updateIncoming({ enabled })}
          />
          <Select
            label="Translate others into"
            value={incoming.targetLanguage}
            options={LANGUAGE_OPTIONS}
            onChange={(targetLanguage) => updateIncoming({ targetLanguage })}
          />
          <TextField
            label="Source app"
            hint="Leave empty to translate the whole system (except LiveDub Voice)."
            placeholder="e.g. Discord.exe"
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
            label="Headphones"
            hint="Where you hear the translation."
            value={incoming.outputDeviceId ?? SYSTEM_DEFAULT_VALUE}
            options={deviceOptions({
              devices,
              kind: "output",
              savedId: incoming.outputDeviceId,
            })}
            onChange={(value) =>
              updateIncoming({ outputDeviceId: deviceIdFromValue(value) })
            }
          />
          <Slider
            direction="incoming"
            label="Original audio volume"
            hint="How loud the original voices stay under the translation."
            value={incoming.originalVolume}
            formatValue={(value) => `${value}%`}
            onChange={(originalVolume) => updateIncoming({ originalVolume })}
          />
        </Card>

        <Card
          direction="outgoing"
          eyebrow="Outgoing"
          title="You → others"
          aria-label="Outgoing (you → others)"
          aside={
            <LanguageRoute
              direction="outgoing"
              size="sm"
              from={sourceLanguageFor(outgoing.targetLanguage)}
              to={outgoing.targetLanguage}
            />
          }
        >
          <Toggle
            direction="outgoing"
            label="Translate your microphone"
            checked={outgoing.enabled}
            onChange={(enabled) => updateOutgoing({ enabled })}
          />
          <Select
            label="Translate you into"
            value={outgoing.targetLanguage}
            options={LANGUAGE_OPTIONS}
            onChange={(targetLanguage) => updateOutgoing({ targetLanguage })}
          />
          <Select
            label="Microphone"
            value={outgoing.inputDeviceId ?? SYSTEM_DEFAULT_VALUE}
            options={deviceOptions({
              devices,
              kind: "input",
              savedId: outgoing.inputDeviceId,
            })}
            onChange={(value) =>
              updateOutgoing({ inputDeviceId: deviceIdFromValue(value) })
            }
          />
          <Select
            label="Virtual cable"
            hint={
              missingCable
                ? "No virtual cable found. Install a virtual cable such as VB-CABLE, then refresh the devices."
                : "Playback side of a virtual cable (e.g. CABLE Input). Pick its recording side as the mic in your call or game."
            }
            value={outgoing.outputDeviceId ?? SYSTEM_DEFAULT_VALUE}
            options={deviceOptions({
              devices,
              kind: "output",
              savedId: outgoing.outputDeviceId,
              cablesFirst: true,
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
          {saving ? "Saving…" : "Save"}
        </Button>
        <Button
          variant="secondary"
          disabled={loadingDevices}
          icon={<RefreshCw size={16} aria-hidden="true" />}
          onClick={refreshDevices}
        >
          {loadingDevices ? "Refreshing…" : "Refresh devices"}
        </Button>
        {devicesError && <Notice tone="error">{devicesError}</Notice>}
        {status && <Notice tone="success">{status}</Notice>}
        {error && <Notice tone="error">{error}</Notice>}
      </div>
    </form>
  );
}

export default SettingsForm;
