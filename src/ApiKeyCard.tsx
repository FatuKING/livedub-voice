import { KeyRound, RefreshCw, Trash2 } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import Button from "./components/Button";
import Card from "./components/Card";
import Notice, { type NoticeTone } from "./components/Notice";
import SecretField from "./components/SecretField";
import StatusPill, { type StatusTone } from "./components/StatusPill";
import {
  deleteApiKey,
  getApiKeyStatus,
  isAppError,
  saveApiKey,
  validateApiKey,
  type ApiKeyStatus,
  type KeyValidation,
} from "./lib/api";
import "./ApiKeyCard.css";

function errorMessage(reason: unknown): string {
  return isAppError(reason) ? reason.message : String(reason);
}

/** What the pill shows: the stored key's state as far as the panel knows. */
type KeyState =
  | { kind: "loading" }
  | { kind: "notSet" }
  | { kind: "checking" }
  | { kind: "checked"; validation: KeyValidation };

const PILLS: Record<
  KeyValidation["kind"],
  { tone: StatusTone; label: string }
> = {
  valid: { tone: "neutral", label: "Valid" },
  invalidKey: { tone: "danger", label: "Invalid" },
  noModelAccess: { tone: "danger", label: "No model access" },
  rateLimited: { tone: "sun", label: "Rate limited" },
  offline: { tone: "sun", label: "Offline" },
  unexpected: { tone: "sun", label: "Could not check" },
};

function pillFor(state: KeyState): { tone: StatusTone; label: string } {
  switch (state.kind) {
    case "loading":
      return { tone: "neutral", label: "Loading" };
    case "notSet":
      return { tone: "sun", label: "Not set" };
    case "checking":
      return { tone: "neutral", label: "Checking" };
    case "checked":
      return PILLS[state.validation.kind];
  }
}

/** Explains a check result and what to do about it. */
function describe(validation: KeyValidation): {
  tone: NoticeTone;
  text: string;
} {
  switch (validation.kind) {
    case "valid":
      return {
        tone: "success",
        text: "OpenAI accepted the key. This check does not spend credits, so a key with no credits also passes: if translation fails later, check billing on platform.openai.com.",
      };
    case "invalidKey":
      return {
        tone: "error",
        text: "OpenAI rejected this key. Check that it was copied completely, or create a new one.",
      };
    case "noModelAccess":
      return {
        tone: "error",
        text: "The key works, but this account cannot use gpt-realtime-translate yet. Make sure billing is set up on platform.openai.com and that the key's project has access to the model.",
      };
    case "rateLimited":
      return {
        tone: "warning",
        text: "OpenAI is rate limiting requests. The key is saved; check again in a minute.",
      };
    case "offline":
      return {
        tone: "warning",
        text: "Could not reach OpenAI. The key is saved; check your connection and try again.",
      };
    case "unexpected":
      return {
        tone: "warning",
        text: `OpenAI answered unexpectedly${
          validation.status == null ? "" : ` (HTTP ${validation.status})`
        }. The key is saved; try again later.`,
      };
  }
}

/**
 * OpenAI API key: save and validate, re-check and remove. The key is stored by the core in
 * Windows Credential Manager; the panel only ever sees its last four characters.
 */
function ApiKeyCard() {
  const [status, setStatus] = useState<ApiKeyStatus | null>(null);
  const [state, setState] = useState<KeyState>({ kind: "loading" });
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [confirmingRemove, setConfirmingRemove] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    getApiKeyStatus()
      .then(async (loaded) => {
        if (!active) return;
        setStatus(loaded);
        if (!loaded.hasKey) {
          setState({ kind: "notSet" });
          return;
        }
        setState({ kind: "checking" });
        const validation = await validateApiKey();
        if (active) setState({ kind: "checked", validation });
      })
      .catch((reason: unknown) => {
        if (!active) return;
        setState((current) =>
          current.kind === "checking" ? { kind: "notSet" } : current,
        );
        setError(`Could not read the API key: ${errorMessage(reason)}`);
      });
    return () => {
      active = false;
    };
  }, []);

  async function handleSave(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (input.trim() === "") return;
    setBusy(true);
    setError(null);
    setConfirmingRemove(false);
    const previous = state;
    setState({ kind: "checking" });
    try {
      const result = await saveApiKey(input);
      setStatus(result.status);
      setState({ kind: "checked", validation: result.validation });
      setInput("");
    } catch (reason: unknown) {
      setState(previous);
      setError(`Could not save the key: ${errorMessage(reason)}`);
    } finally {
      setBusy(false);
    }
  }

  async function handleCheck() {
    setBusy(true);
    setError(null);
    setState({ kind: "checking" });
    try {
      setState({ kind: "checked", validation: await validateApiKey() });
    } catch (reason: unknown) {
      setState({ kind: "notSet" });
      setError(`Could not check the key: ${errorMessage(reason)}`);
    } finally {
      setBusy(false);
    }
  }

  async function handleRemove() {
    setBusy(true);
    setError(null);
    try {
      setStatus(await deleteApiKey());
      setState({ kind: "notSet" });
      setConfirmingRemove(false);
    } catch (reason: unknown) {
      setError(`Could not remove the key: ${errorMessage(reason)}`);
    } finally {
      setBusy(false);
    }
  }

  const pill = pillFor(state);
  const result = state.kind === "checked" ? describe(state.validation) : null;
  const hasKey = status?.hasKey ?? false;

  return (
    <Card
      className="ld-api-key"
      eyebrow="OpenAI"
      title="API key"
      aria-label="OpenAI API key"
      aside={<StatusPill tone={pill.tone}>{pill.label}</StatusPill>}
    >
      <p className="ld-api-key__intro">
        Create a key at platform.openai.com → API keys. LiveDub Voice stores it
        in Windows Credential Manager and only sends it to OpenAI. Translation
        costs about $0.034 per audio minute for each active direction (≈ $4/hour
        with both on), billed to your OpenAI account.
      </p>

      {hasKey && status?.last4 != null && (
        <p className="ld-api-key__saved">
          Saved key: <span className="mono">sk-…{status.last4}</span>
        </p>
      )}

      {result && <Notice tone={result.tone}>{result.text}</Notice>}

      <form
        className="ld-api-key__form"
        onSubmit={(event) => void handleSave(event)}
      >
        <SecretField
          label={hasKey ? "Replace API key" : "API key"}
          placeholder="sk-…"
          value={input}
          onChange={(event) => setInput(event.target.value)}
        />
        <div className="ld-api-key__actions">
          <Button
            type="submit"
            disabled={busy || input.trim() === ""}
            icon={<KeyRound size={16} aria-hidden="true" />}
          >
            Save and validate
          </Button>
          {hasKey && !confirmingRemove && (
            <>
              <Button
                variant="secondary"
                disabled={busy}
                icon={<RefreshCw size={16} aria-hidden="true" />}
                onClick={() => void handleCheck()}
              >
                Check again
              </Button>
              <Button
                variant="ghost"
                disabled={busy}
                icon={<Trash2 size={16} aria-hidden="true" />}
                onClick={() => setConfirmingRemove(true)}
              >
                Remove
              </Button>
            </>
          )}
        </div>
      </form>

      {confirmingRemove && (
        <div className="ld-api-key__confirm" role="group" aria-label="Confirm">
          <Notice tone="warning">
            Remove the saved key from this PC? Translation stops working until
            you add a key again.
          </Notice>
          <div className="ld-api-key__actions">
            <Button
              variant="secondary"
              disabled={busy}
              icon={<Trash2 size={16} aria-hidden="true" />}
              onClick={() => void handleRemove()}
            >
              Yes, remove key
            </Button>
            <Button
              variant="ghost"
              disabled={busy}
              onClick={() => setConfirmingRemove(false)}
            >
              Cancel
            </Button>
          </div>
        </div>
      )}

      {error && <Notice tone="error">{error}</Notice>}
    </Card>
  );
}

export default ApiKeyCard;
