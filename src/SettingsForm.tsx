import { Save } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import Button from "./components/Button";
import Card from "./components/Card";
import LanguageRoute from "./components/LanguageRoute";
import Notice from "./components/Notice";
import Select from "./components/Select";
import Slider from "./components/Slider";
import TextField from "./components/TextField";
import Toggle from "./components/Toggle";
import {
  getSettings,
  isAppError,
  saveSettings,
  type Settings,
} from "./lib/api";
import { LANGUAGE_OPTIONS, sourceLanguageFor } from "./lib/languages";
import "./SettingsForm.css";

function errorMessage(reason: unknown): string {
  return isAppError(reason) ? reason.message : String(reason);
}

/** Empty text field → `null` (default device). */
function optionalText(value: string): string | null {
  return value === "" ? null : value;
}

/**
 * Settings form: languages, ducking, enabled toggles, devices and source app, grouped per direction.
 *
 * Devices and the source app are plain text fields until real device/process listing exists.
 */
function SettingsForm() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

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
          <TextField
            label="Headphones device id"
            hint="Leave empty to use the default playback device."
            value={incoming.outputDeviceId ?? ""}
            onChange={(event) =>
              updateIncoming({
                outputDeviceId: optionalText(event.target.value),
              })
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
          <TextField
            label="Microphone device id"
            hint="Leave empty to use the default microphone."
            value={outgoing.inputDeviceId ?? ""}
            onChange={(event) =>
              updateOutgoing({
                inputDeviceId: optionalText(event.target.value),
              })
            }
          />
          <TextField
            label="Virtual cable device id"
            hint="Playback side of a virtual cable (e.g. VB-CABLE). Pick its recording side as the mic in your call or game."
            value={outgoing.outputDeviceId ?? ""}
            onChange={(event) =>
              updateOutgoing({
                outputDeviceId: optionalText(event.target.value),
              })
            }
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
        {status && <Notice tone="success">{status}</Notice>}
        {error && <Notice tone="error">{error}</Notice>}
      </div>
    </form>
  );
}

export default SettingsForm;
