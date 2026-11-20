import { Play, Square } from "lucide-react";
import { useEffect, useId, useState } from "react";
import Button from "./components/Button";
import LevelMeter from "./components/LevelMeter";
import Notice from "./components/Notice";
import StatusPill, { type StatusTone } from "./components/StatusPill";
import VoiceIndicator from "./components/VoiceIndicator";
import { useT } from "./i18n/context";
import { translateError } from "./i18n/errors";
import {
  isAppError,
  overrideSpendingCap,
  type InputLevel,
  type PipelineState,
  type PipelineStatus,
  type VoiceActivity,
} from "./lib/api";
import type { Direction } from "./lib/languages";
import { PIPELINES, isActive } from "./lib/pipelines";
import "./TranslationControls.css";

const SILENCE: InputLevel = { rms: 0, peak: 0 };
const IDLE: PipelineStatus = { state: "idle", error: null, reconnect: null };
const QUIET: VoiceActivity = { speaking: false, sending: false };

/** Translated text kept for the caption area (the tail of the session). */
const MAX_CAPTION_CHARS = 280;

function tones(direction: Direction): Record<PipelineState, StatusTone> {
  return {
    idle: "neutral",
    connecting: "sun",
    live: direction,
    reconnecting: "sun",
    stopping: "sun",
    error: "danger",
  };
}

export interface TranslationControlsProps {
  /** Incoming (others → you, teal) or outgoing (you → others, coral). */
  direction: Direction;
  /** Why Start is disabled (shown with a link to Settings); `null` when it can start. */
  blocked: string | null;
  /** A warning that does not block Start (for example: the output is not a virtual cable). */
  warning?: string | null;
  /** How to use this direction, under the captions. */
  help: string;
  onOpenSettings: () => void;
  /** Opens the Usage screen (where the spending cap is raised). */
  onOpenUsage?: () => void;
}

/**
 * Start/stop, status, input level and live captions of one translation direction. The
 * translation keeps running in the core when this unmounts.
 */
function TranslationControls({
  direction,
  blocked,
  warning = null,
  help,
  onOpenSettings,
  onOpenUsage,
}: TranslationControlsProps) {
  const t = useT();
  const noteId = useId();
  const captionsId = useId();
  const [status, setStatus] = useState<PipelineStatus>(IDLE);
  const [level, setLevel] = useState<InputLevel>(SILENCE);
  const [caption, setCaption] = useState("");
  const [voice, setVoice] = useState<VoiceActivity>(QUIET);
  /** Minutes without speech after which the direction turned itself off, until the next start. */
  const [autoOff, setAutoOff] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  /** Start was refused because the spending cap is reached: ask before going on. */
  const [capBlocked, setCapBlocked] = useState(false);
  const pipeline = PIPELINES[direction];

  useEffect(() => {
    let current = true;
    function applyStatus(next: PipelineStatus) {
      if (!current) return;
      setStatus(next);
      if (next.state !== "live") {
        setLevel(SILENCE);
        setVoice(QUIET);
      }
    }
    const subscriptions = [
      pipeline.onStatus((next) => {
        if (next.state === "connecting") {
          setCaption("");
          setAutoOff(null);
        }
        applyStatus(next);
      }),
      pipeline.onLevel((next) => {
        if (current) setLevel(next);
      }),
      pipeline.onTranscript((delta) => {
        if (!current) return;
        setCaption((text) => (text + delta).slice(-MAX_CAPTION_CHARS));
      }),
      pipeline.onVoice((next) => {
        if (current) setVoice(next);
      }),
      pipeline.onAutoOff((minutes) => {
        if (current) setAutoOff(minutes);
      }),
    ];
    pipeline
      .getStatus()
      .then(applyStatus)
      .catch(() => undefined);
    return () => {
      current = false;
      for (const subscription of subscriptions) {
        subscription.then((unlisten) => unlisten()).catch(() => undefined);
      }
    };
  }, [pipeline]);

  const stopping = status.state === "stopping";
  const running = isActive(status.state);
  const reconnect = status.state === "reconnecting" ? status.reconnect : null;

  async function toggle(overrideCap = false) {
    setBusy(true);
    setCapBlocked(false);
    try {
      if (running) {
        // The core finishes the last sentence first (up to ~10 s).
        setStatus({ state: "stopping", error: null, reconnect: null });
        await pipeline.stop();
        setStatus(IDLE);
      } else {
        if (overrideCap) await overrideSpendingCap();
        setCaption("");
        setAutoOff(null);
        setStatus({ state: "connecting", error: null, reconnect: null });
        await pipeline.start();
      }
    } catch (reason: unknown) {
      if (isAppError(reason) && reason.code === "spending_cap_reached") {
        // Nothing started: ask whether to go on instead of showing an error.
        setStatus(IDLE);
        setCapBlocked(true);
        return;
      }
      setStatus({
        state: "error",
        error: isAppError(reason)
          ? reason
          : { code: "internal", message: String(reason) },
        reconnect: null,
      });
    } finally {
      setBusy(false);
    }
  }

  const startBlocked = !running && blocked !== null;

  return (
    <div className="ld-translation">
      <div className="ld-route-card__buttons">
        <Button
          variant={running ? "secondary" : direction}
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
          {running ? t(`home.${direction}.stop`) : t(`home.${direction}.start`)}
        </Button>
        <StatusPill tone={tones(direction)[status.state]}>
          {reconnect
            ? t("home.status.reconnectingAttempt", {
                attempt: reconnect.attempt,
                max: reconnect.maxAttempts,
              })
            : t(`home.status.${status.state}`)}
        </StatusPill>
      </div>

      {!running && blocked && (
        <p className="ld-route-card__note" id={noteId}>
          {blocked}{" "}
          <button
            type="button"
            className="ld-translation__link"
            onClick={onOpenSettings}
          >
            {t("home.goToSettings")}
          </button>
        </p>
      )}
      {capBlocked && !running && (
        <div
          className="ld-translation__confirm"
          role="group"
          aria-label={t("cap.startBlockedLabel")}
        >
          <Notice tone="warning">{t("cap.startBlocked")}</Notice>
          <div className="ld-route-card__buttons">
            <Button
              variant={direction}
              disabled={busy}
              onClick={() => void toggle(true)}
            >
              {t("cap.continueToday")}
            </Button>
            {onOpenUsage && (
              <Button variant="secondary" onClick={onOpenUsage}>
                {t("cap.raise")}
              </Button>
            )}
            <Button variant="ghost" onClick={() => setCapBlocked(false)}>
              {t("cap.cancel")}
            </Button>
          </div>
        </div>
      )}
      {!blocked && warning && <Notice tone="warning">{warning}</Notice>}
      {status.state === "reconnecting" && (
        <Notice tone="warning">{t(`home.${direction}.reconnecting`)}</Notice>
      )}
      {autoOff !== null && !running && status.state !== "error" && (
        <Notice tone="info">
          {t(`home.${direction}.autoOff`, { count: autoOff })}
        </Notice>
      )}
      {status.state === "error" && status.error && (
        <Notice tone="error">
          {t(`home.${direction}.stopped`, {
            error: translateError(status.error, t),
          })}
        </Notice>
      )}

      <LevelMeter
        direction={direction}
        label={t(`home.${direction}.level`)}
        level={level.rms}
        peak={level.peak}
      />
      {status.state === "live" && (
        <VoiceIndicator
          direction={direction}
          speaking={voice.speaking}
          name={t("home.voice.label")}
          label={t(
            voice.speaking ? "home.voice.speaking" : "home.voice.listening",
          )}
          hint={
            voice.sending
              ? t("home.voice.sendingHint")
              : t("home.voice.pausedHint")
          }
        />
      )}

      <div className="ld-translation__captions">
        <span className="ld-translation__captions-label" id={captionsId}>
          {t(`home.${direction}.captions`)}
        </span>
        <p
          className={`ld-translation__caption ld-translation__caption--${direction}`}
          aria-labelledby={captionsId}
          aria-live="polite"
        >
          {caption || (
            <span className="ld-translation__caption-empty">
              {t(`home.${direction}.captionsEmpty`)}
            </span>
          )}
        </p>
      </div>

      <p className="ld-route-card__note">{help}</p>
    </div>
  );
}

export default TranslationControls;
