import { Play, Square } from "lucide-react";
import { useEffect, useId, useState } from "react";
import Button from "./components/Button";
import LevelMeter from "./components/LevelMeter";
import Notice from "./components/Notice";
import StatusPill, { type StatusTone } from "./components/StatusPill";
import { useT } from "./i18n/context";
import { translateError } from "./i18n/errors";
import {
  getOutgoingStatus,
  isAppError,
  onOutgoingLevel,
  onOutgoingStatus,
  onOutgoingTranscript,
  startOutgoing,
  stopOutgoing,
  type InputLevel,
  type PipelineState,
  type PipelineStatus,
} from "./lib/api";
import "./OutgoingControls.css";

const SILENCE: InputLevel = { rms: 0, peak: 0 };
const IDLE: PipelineStatus = { state: "idle", error: null };

/** Translated text kept for the caption area (the tail of the session). */
const MAX_CAPTION_CHARS = 280;

const TONES: Record<PipelineState, StatusTone> = {
  idle: "neutral",
  connecting: "sun",
  live: "outgoing",
  stopping: "sun",
  error: "danger",
};

/** Why Start is disabled: a missing API key, no output chosen, or the chosen output is gone. */
export type OutgoingBlocker = "noApiKey" | "noOutput" | "outputMissing";

export interface OutgoingControlsProps {
  /** Set when the outgoing translation cannot start; Start is disabled and the reason shown. */
  blocker: OutgoingBlocker | null;
  /** Whether the chosen output is a known virtual cable; a warning is shown when it is not. */
  outputIsCable: boolean;
  onOpenSettings: () => void;
}

/**
 * Start/stop, status, microphone level and live captions of the outgoing translation
 * (microphone → virtual cable). The translation keeps running in the core when this unmounts.
 */
function OutgoingControls({
  blocker,
  outputIsCable,
  onOpenSettings,
}: OutgoingControlsProps) {
  const t = useT();
  const noteId = useId();
  const captionsId = useId();
  const [status, setStatus] = useState<PipelineStatus>(IDLE);
  const [level, setLevel] = useState<InputLevel>(SILENCE);
  const [caption, setCaption] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let current = true;
    function applyStatus(next: PipelineStatus) {
      if (!current) return;
      setStatus(next);
      if (next.state !== "live") setLevel(SILENCE);
    }
    const subscriptions = [
      onOutgoingStatus((next) => {
        if (next.state === "connecting") setCaption("");
        applyStatus(next);
      }),
      onOutgoingLevel((next) => {
        if (current) setLevel(next);
      }),
      onOutgoingTranscript((delta) => {
        if (!current) return;
        setCaption((text) => (text + delta).slice(-MAX_CAPTION_CHARS));
      }),
    ];
    getOutgoingStatus()
      .then(applyStatus)
      .catch(() => undefined);
    return () => {
      current = false;
      for (const subscription of subscriptions) {
        subscription.then((unlisten) => unlisten()).catch(() => undefined);
      }
    };
  }, []);

  const stopping = status.state === "stopping";
  const running =
    status.state === "connecting" || status.state === "live" || stopping;

  async function toggle() {
    setBusy(true);
    try {
      if (running) {
        // The core finishes the last sentence first (up to ~10 s).
        setStatus({ state: "stopping", error: null });
        await stopOutgoing();
        setStatus(IDLE);
      } else {
        setCaption("");
        setStatus({ state: "connecting", error: null });
        await startOutgoing();
      }
    } catch (reason: unknown) {
      setStatus({
        state: "error",
        error: isAppError(reason)
          ? reason
          : { code: "internal", message: String(reason) },
      });
    } finally {
      setBusy(false);
    }
  }

  const startBlocked = !running && blocker !== null;

  return (
    <div className="ld-outgoing">
      <div className="ld-route-card__buttons">
        <Button
          variant={running ? "secondary" : "outgoing"}
          icon={
            running ? (
              <Square size={16} aria-hidden="true" />
            ) : (
              <Play size={16} aria-hidden="true" />
            )
          }
          disabled={busy || stopping || startBlocked}
          aria-describedby={startBlocked ? noteId : undefined}
          onClick={() => void toggle()}
        >
          {running ? t("home.outgoing.stop") : t("home.outgoing.start")}
        </Button>
        <StatusPill tone={TONES[status.state]}>
          {t(`home.status.${status.state}`)}
        </StatusPill>
      </div>

      {!running && blocker && (
        <p className="ld-route-card__note" id={noteId}>
          {t(`home.outgoing.blocked.${blocker}`)}{" "}
          <button
            type="button"
            className="ld-outgoing__link"
            onClick={onOpenSettings}
          >
            {t("home.outgoing.goToSettings")}
          </button>
        </p>
      )}
      {!blocker && !outputIsCable && (
        <Notice tone="warning">{t("home.outgoing.notCable")}</Notice>
      )}
      {status.state === "error" && status.error && (
        <Notice tone="error">
          {t("home.outgoing.stopped", {
            error: translateError(status.error, t),
          })}
        </Notice>
      )}

      <LevelMeter
        direction="outgoing"
        label={t("home.outgoing.level")}
        level={level.rms}
        peak={level.peak}
      />

      <div className="ld-outgoing__captions">
        <span className="ld-outgoing__captions-label" id={captionsId}>
          {t("home.outgoing.captions")}
        </span>
        <p
          className="ld-outgoing__caption"
          aria-labelledby={captionsId}
          aria-live="polite"
        >
          {caption || (
            <span className="ld-outgoing__caption-empty">
              {t("home.outgoing.captionsEmpty")}
            </span>
          )}
        </p>
      </div>

      <p className="ld-route-card__note">{t("home.outgoing.help")}</p>
    </div>
  );
}

export default OutgoingControls;
