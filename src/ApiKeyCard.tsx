import { KeyRound, RefreshCw, Trash2 } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import Button from "./components/Button";
import Card from "./components/Card";
import Notice, { type NoticeTone } from "./components/Notice";
import SecretField from "./components/SecretField";
import StatusPill, { type StatusTone } from "./components/StatusPill";
import { useI18n } from "./i18n/context";
import { failureText, type Failure } from "./i18n/errors";
import type { Translate } from "./i18n/translate";
import {
  deleteApiKey,
  getApiKeyStatus,
  saveApiKey,
  validateApiKey,
  type ApiKeyStatus,
  type KeyValidation,
} from "./lib/api";
import { PRICE_PER_MINUTE_USD } from "./lib/pricing";
import "./ApiKeyCard.css";

/** What the pill shows: the stored key's state as far as the panel knows. */
type KeyState =
  | { kind: "loading" }
  | { kind: "notSet" }
  | { kind: "checking" }
  | { kind: "checked"; validation: KeyValidation };

const PILL_TONES: Record<KeyValidation["kind"], StatusTone> = {
  valid: "neutral",
  invalidKey: "danger",
  noModelAccess: "danger",
  rateLimited: "sun",
  offline: "sun",
  unexpected: "sun",
};

function pillFor(
  state: KeyState,
  t: Translate,
): { tone: StatusTone; label: string } {
  switch (state.kind) {
    case "loading":
      return { tone: "neutral", label: t("apiKey.pill.loading") };
    case "notSet":
      return { tone: "sun", label: t("apiKey.pill.notSet") };
    case "checking":
      return { tone: "neutral", label: t("apiKey.pill.checking") };
    case "checked": {
      const kind = state.validation.kind;
      return { tone: PILL_TONES[kind], label: t(`apiKey.pill.${kind}`) };
    }
  }
}

const RESULT_TONES: Record<KeyValidation["kind"], NoticeTone> = {
  valid: "success",
  invalidKey: "error",
  noModelAccess: "error",
  rateLimited: "warning",
  offline: "warning",
  unexpected: "warning",
};

/** Explains a check result and what to do about it. */
function describe(
  validation: KeyValidation,
  t: Translate,
): { tone: NoticeTone; text: string } {
  const tone = RESULT_TONES[validation.kind];
  if (validation.kind === "unexpected" && validation.status != null) {
    return {
      tone,
      text: t("apiKey.result.unexpectedStatus", {
        status: String(validation.status),
      }),
    };
  }
  return { tone, text: t(`apiKey.result.${validation.kind}`) };
}

/**
 * OpenAI API key: save and validate, re-check and remove. The key is stored by the core in
 * Windows Credential Manager; the panel only ever sees its last four characters.
 */
function ApiKeyCard() {
  const { t, rich, format } = useI18n();
  const [status, setStatus] = useState<ApiKeyStatus | null>(null);
  const [state, setState] = useState<KeyState>({ kind: "loading" });
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [confirmingRemove, setConfirmingRemove] = useState(false);
  const [error, setError] = useState<Failure | null>(null);

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
        setError({ key: "apiKey.readError", reason });
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
      setError({ key: "apiKey.saveError", reason });
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
      setError({ key: "apiKey.checkError", reason });
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
      setError({ key: "apiKey.removeError", reason });
    } finally {
      setBusy(false);
    }
  }

  const pill = pillFor(state, t);
  const result =
    state.kind === "checked" ? describe(state.validation, t) : null;
  const hasKey = status?.hasKey ?? false;

  return (
    <Card
      className="ld-api-key"
      eyebrow={t("apiKey.eyebrow")}
      title={t("apiKey.title")}
      aria-label={t("apiKey.ariaLabel")}
      aside={<StatusPill tone={pill.tone}>{pill.label}</StatusPill>}
    >
      <p className="ld-api-key__intro">
        {t("apiKey.intro", {
          price: format.currency(PRICE_PER_MINUTE_USD, {
            maximumFractionDigits: 3,
          }),
          // Both directions on: 2 × 60 minutes.
          hourly: format.currency(PRICE_PER_MINUTE_USD * 120, {
            maximumFractionDigits: 0,
          }),
        })}
      </p>

      {hasKey && status?.last4 != null && (
        <p className="ld-api-key__saved">
          {rich("apiKey.savedKey", {
            key: <span className="mono">sk-…{status.last4}</span>,
          })}
        </p>
      )}

      {result && <Notice tone={result.tone}>{result.text}</Notice>}

      <form
        className="ld-api-key__form"
        onSubmit={(event) => void handleSave(event)}
      >
        <SecretField
          label={hasKey ? t("apiKey.replaceLabel") : t("apiKey.label")}
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
            {t("apiKey.save")}
          </Button>
          {hasKey && !confirmingRemove && (
            <>
              <Button
                variant="secondary"
                disabled={busy}
                icon={<RefreshCw size={16} aria-hidden="true" />}
                onClick={() => void handleCheck()}
              >
                {t("apiKey.checkAgain")}
              </Button>
              <Button
                variant="ghost"
                disabled={busy}
                icon={<Trash2 size={16} aria-hidden="true" />}
                onClick={() => setConfirmingRemove(true)}
              >
                {t("apiKey.remove")}
              </Button>
            </>
          )}
        </div>
      </form>

      {confirmingRemove && (
        <div
          className="ld-api-key__confirm"
          role="group"
          aria-label={t("apiKey.confirmLabel")}
        >
          <Notice tone="warning">{t("apiKey.confirmText")}</Notice>
          <div className="ld-api-key__actions">
            <Button
              variant="secondary"
              disabled={busy}
              icon={<Trash2 size={16} aria-hidden="true" />}
              onClick={() => void handleRemove()}
            >
              {t("apiKey.confirmRemove")}
            </Button>
            <Button
              variant="ghost"
              disabled={busy}
              onClick={() => setConfirmingRemove(false)}
            >
              {t("apiKey.cancel")}
            </Button>
          </div>
        </div>
      )}

      {error && <Notice tone="error">{failureText(error, t)}</Notice>}
    </Card>
  );
}

export default ApiKeyCard;
