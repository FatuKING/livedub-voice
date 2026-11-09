import { Mic, MicOff } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import Button from "./components/Button";
import LevelMeter from "./components/LevelMeter";
import Notice from "./components/Notice";
import Toggle from "./components/Toggle";
import { useT } from "./i18n/context";
import { translateError } from "./i18n/errors";
import {
  onMonitorLevel,
  onMonitorStopped,
  startMonitor,
  stopMonitor,
  type InputLevel,
} from "./lib/api";
import "./MicTest.css";

const SILENCE: InputLevel = { rms: 0, peak: 0 };

export interface MicTestProps {
  /** Microphone to test; `null` = system default. */
  inputId: string | null;
  /** Where "Listen to myself" plays the microphone; `null` = system default. */
  outputId: string | null;
}

/**
 * Microphone test: a live level meter and an optional "Listen to myself" loopback.
 *
 * Follows the devices picked in the form (even before saving) and restarts when they change.
 */
function MicTest({ inputId, outputId }: MicTestProps) {
  const [active, setActive] = useState(false);
  const [listen, setListen] = useState(false);
  const [level, setLevel] = useState<InputLevel>(SILENCE);
  const t = useT();
  /** Why the test ended: a failed start, or the core's message when it stopped on its own. */
  const [error, setError] = useState<
    | { kind: "failed"; reason: unknown }
    | { kind: "stopped"; detail: string }
    | null
  >(null);

  // Start/stop calls run one after another so a quick stop never overtakes a slow start.
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const enqueue = useCallback(<T,>(operation: () => Promise<T>): Promise<T> => {
    const next = queue.current.then(operation, operation);
    queue.current = next.catch(() => undefined);
    return next;
  }, []);

  useEffect(() => {
    if (!active) return;
    const subscriptions = [
      onMonitorLevel(setLevel),
      onMonitorStopped((detail) => {
        setError({ kind: "stopped", detail });
        setActive(false);
      }),
    ];
    return () => {
      setLevel(SILENCE);
      for (const subscription of subscriptions) {
        void subscription.then((unlisten) => unlisten());
      }
      void enqueue(stopMonitor).catch(() => undefined);
    };
  }, [active, enqueue]);

  useEffect(() => {
    if (!active) return;
    let current = true;
    enqueue(() => startMonitor(inputId, outputId, listen)).catch(
      (reason: unknown) => {
        if (!current) return;
        setError({ kind: "failed", reason });
        setActive(false);
      },
    );
    return () => {
      current = false;
    };
  }, [active, inputId, outputId, listen, enqueue]);

  function toggleTest() {
    setError(null);
    setActive((value) => !value);
  }

  return (
    <div
      className="ld-mic-test"
      role="group"
      aria-label={t("micTest.groupLabel")}
    >
      <div className="ld-mic-test__header">
        <span className="ld-mic-test__title">{t("micTest.title")}</span>
        <Button
          variant={active ? "secondary" : "outgoing"}
          icon={
            active ? (
              <MicOff size={16} aria-hidden="true" />
            ) : (
              <Mic size={16} aria-hidden="true" />
            )
          }
          onClick={toggleTest}
        >
          {active ? t("micTest.stop") : t("micTest.start")}
        </Button>
      </div>
      <LevelMeter
        direction="outgoing"
        label={t("micTest.level")}
        level={level.rms}
        peak={level.peak}
      />
      <Toggle
        direction="outgoing"
        label={t("micTest.listen")}
        hint={t("micTest.listenHint")}
        checked={listen}
        onChange={(checked) => {
          setError(null);
          setListen(checked);
        }}
      />
      {error && (
        <Notice tone="error">
          {error.kind === "failed"
            ? translateError(error.reason, t)
            : t("micTest.stopped", { detail: error.detail })}
        </Notice>
      )}
    </div>
  );
}

export default MicTest;
