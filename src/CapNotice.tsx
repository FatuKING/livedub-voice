import { useState } from "react";
import Button from "./components/Button";
import Notice from "./components/Notice";
import { useI18n } from "./i18n/context";
import { failureText, type Failure } from "./i18n/errors";
import {
  overrideSpendingCap,
  type CapStatus,
  type UsageSnapshot,
} from "./lib/api";
import { formatCost } from "./lib/usage";
import "./CapNotice.css";

export interface CapNoticeProps {
  cap: CapStatus;
  /** The cap just stopped the translations (says "stopped" instead of "reached"). */
  stopped?: boolean;
  /** Opens the screen where the cap is edited; omitted there. */
  onRaise?: () => void;
  /** The user chose to keep going today; receives the new usage. */
  onContinued: (usage: UsageSnapshot) => void;
}

/**
 * The spending cap's notice: a warning from 80 %, and once reached, why translations are off
 * with "Keep going today" and "Raise the cap". Renders nothing under 80 % or without a cap.
 */
function CapNotice({
  cap,
  stopped = false,
  onRaise,
  onContinued,
}: CapNoticeProps) {
  const { t, format } = useI18n();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<Failure | null>(null);

  if (cap.period === null || cap.limitUsd === null) return null;
  if (cap.level !== "warning" && cap.level !== "reached") return null;

  const params = {
    period: t(`cap.period.${cap.period}`),
    spent: formatCost(format, cap.spentUsd),
    limit: formatCost(format, cap.limitUsd),
    percent: format.percent(
      Math.floor((cap.spentUsd / cap.limitUsd) * 100) / 100,
    ),
  };

  if (cap.level === "warning") {
    return <Notice tone="warning">{t("cap.warning", params)}</Notice>;
  }
  if (cap.overriddenToday) {
    return <Notice tone="info">{t("cap.overridden", params)}</Notice>;
  }

  async function keepGoing() {
    setBusy(true);
    setError(null);
    try {
      onContinued(await overrideSpendingCap());
    } catch (reason: unknown) {
      setError({ key: "cap.continueError", reason });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="ld-cap-notice">
      <Notice tone="error">
        {t(stopped ? "cap.stopped" : "cap.reached", params)}
      </Notice>
      <div className="ld-cap-notice__actions">
        <Button
          variant="secondary"
          disabled={busy}
          onClick={() => void keepGoing()}
        >
          {t("cap.continueToday")}
        </Button>
        {onRaise && (
          <Button variant="ghost" onClick={onRaise}>
            {t("cap.raise")}
          </Button>
        )}
      </div>
      {error && <Notice tone="error">{failureText(error, t)}</Notice>}
    </div>
  );
}

export default CapNotice;
