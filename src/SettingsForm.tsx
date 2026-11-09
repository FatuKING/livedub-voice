import { useEffect, useState, type FormEvent } from "react";
import {
  getSettings,
  isAppError,
  saveSettings,
  type Language,
  type Settings,
} from "./lib/api";

const LANGUAGES: { value: Language; label: string }[] = [
  { value: "es", label: "Spanish" },
  { value: "en", label: "English" },
];

function errorMessage(reason: unknown): string {
  return isAppError(reason) ? reason.message : String(reason);
}

/** Empty text field → `null` (default device). */
function optionalText(value: string): string | null {
  return value === "" ? null : value;
}

/**
 * Minimal settings form: languages, ducking, enabled toggles, devices and source app.
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
    return error ? <p role="alert">{error}</p> : <p>Loading settings…</p>;
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
    <form onSubmit={(event) => void handleSubmit(event)}>
      <fieldset>
        <legend>Incoming (others → you)</legend>
        <label>
          <input
            type="checkbox"
            checked={incoming.enabled}
            onChange={(event) =>
              updateIncoming({ enabled: event.target.checked })
            }
          />
          Translate incoming audio
        </label>
        <label>
          Translate others into
          <select
            value={incoming.targetLanguage}
            onChange={(event) =>
              updateIncoming({
                targetLanguage: event.target.value as Language,
              })
            }
          >
            {LANGUAGES.map(({ value, label }) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label>
          Source app (empty = whole system)
          <input
            type="text"
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
        </label>
        <label>
          Headphones device id (empty = default)
          <input
            type="text"
            value={incoming.outputDeviceId ?? ""}
            onChange={(event) =>
              updateIncoming({
                outputDeviceId: optionalText(event.target.value),
              })
            }
          />
        </label>
        <label>
          Original audio volume: {incoming.originalVolume}%
          <input
            type="range"
            min={0}
            max={100}
            value={incoming.originalVolume}
            onChange={(event) =>
              updateIncoming({ originalVolume: Number(event.target.value) })
            }
          />
        </label>
      </fieldset>

      <fieldset>
        <legend>Outgoing (you → others)</legend>
        <label>
          <input
            type="checkbox"
            checked={outgoing.enabled}
            onChange={(event) =>
              updateOutgoing({ enabled: event.target.checked })
            }
          />
          Translate your microphone
        </label>
        <label>
          Translate you into
          <select
            value={outgoing.targetLanguage}
            onChange={(event) =>
              updateOutgoing({
                targetLanguage: event.target.value as Language,
              })
            }
          >
            {LANGUAGES.map(({ value, label }) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
        <label>
          Microphone device id (empty = default)
          <input
            type="text"
            value={outgoing.inputDeviceId ?? ""}
            onChange={(event) =>
              updateOutgoing({
                inputDeviceId: optionalText(event.target.value),
              })
            }
          />
        </label>
        <label>
          Virtual cable device id
          <input
            type="text"
            value={outgoing.outputDeviceId ?? ""}
            onChange={(event) =>
              updateOutgoing({
                outputDeviceId: optionalText(event.target.value),
              })
            }
          />
        </label>
      </fieldset>

      <button type="submit" disabled={saving}>
        {saving ? "Saving…" : "Save"}
      </button>
      {status && <p role="status">{status}</p>}
      {error && <p role="alert">{error}</p>}
    </form>
  );
}

export default SettingsForm;
