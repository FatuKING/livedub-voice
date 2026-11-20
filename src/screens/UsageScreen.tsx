import { ChartColumn } from "lucide-react";
import CapNotice from "../CapNotice";
import Card from "../components/Card";
import Notice from "../components/Notice";
import PageHeader from "../components/PageHeader";
import StatusPill from "../components/StatusPill";
import { useI18n } from "../i18n/context";
import { failureText } from "../i18n/errors";
import type { PeriodUsage, SessionUsage } from "../lib/api";
import type { Direction } from "../lib/languages";
import { PRICE_PER_MINUTE_USD } from "../lib/pricing";
import { formatCost, formatMinutes, localDay } from "../lib/usage";
import SpendingCapCard from "../SpendingCapCard";
import { useUsage } from "../useUsage";
import "./screens.css";

function SessionCard({
  direction,
  session,
}: {
  direction: Direction;
  session: SessionUsage | null;
}) {
  const { t, format } = useI18n();
  const started = session !== null && (session.active || session.seconds > 0);
  return (
    <Card
      direction={direction}
      eyebrow={t("usage.session.eyebrow")}
      title={t(`usage.session.${direction}`)}
      aside={
        started && (
          <StatusPill tone={session.active ? direction : "neutral"}>
            {session.active ? t("usage.session.live") : t("usage.session.last")}
          </StatusPill>
        )
      }
    >
      {started ? (
        <dl className="ld-pass">
          <div className="ld-pass__item">
            <dt>{t("usage.session.minutes")}</dt>
            <dd className="mono">{formatMinutes(format, session.seconds)}</dd>
          </div>
          <div className="ld-pass__item">
            <dt>{t("usage.session.cost")}</dt>
            <dd className="mono">{formatCost(format, session.costUsd)}</dd>
          </div>
        </dl>
      ) : (
        <p className="ld-route-card__note">{t("usage.session.none")}</p>
      )}
    </Card>
  );
}

function PeriodTotals({ label, usage }: { label: string; usage: PeriodUsage }) {
  const { t, format } = useI18n();
  const minutes = (seconds: number) =>
    t("usage.minutesShort", { minutes: formatMinutes(format, seconds) });
  return (
    <div className="ld-usage-period">
      <h3 className="ld-usage-period__title">{label}</h3>
      <dl className="ld-pass ld-pass--three">
        <div className="ld-pass__item">
          <dt>{t("usage.totals.incoming")}</dt>
          <dd className="mono">{minutes(usage.incomingSeconds)}</dd>
        </div>
        <div className="ld-pass__item">
          <dt>{t("usage.totals.outgoing")}</dt>
          <dd className="mono">{minutes(usage.outgoingSeconds)}</dd>
        </div>
        <div className="ld-pass__item">
          <dt>{t("usage.totals.cost")}</dt>
          <dd className="mono">{formatCost(format, usage.costUsd)}</dd>
        </div>
      </dl>
    </div>
  );
}

/** Minutes translated and estimated cost (session, today, month, per day) and the cap. */
function UsageScreen() {
  const { t, rich, format } = useI18n();
  const { usage, error, stoppedBy, setUsage } = useUsage();
  const price = (
    <span className="mono">
      {format.currency(usage?.pricePerMinuteUsd ?? PRICE_PER_MINUTE_USD, {
        maximumFractionDigits: 3,
      })}
    </span>
  );

  return (
    <>
      <PageHeader
        title={t("usage.title")}
        description={t("usage.description")}
      />
      <Notice tone="info">{rich("usage.estimate", { price })}</Notice>
      {error !== null && (
        <Notice tone="error">
          {failureText({ key: "usage.loadError", reason: error }, t)}
        </Notice>
      )}
      {usage && (
        <CapNotice
          cap={usage.cap}
          stopped={stoppedBy !== null}
          onContinued={setUsage}
        />
      )}

      <div className="ld-route-grid">
        <SessionCard
          direction="incoming"
          session={usage?.session.incoming ?? null}
        />
        <SessionCard
          direction="outgoing"
          session={usage?.session.outgoing ?? null}
        />
      </div>

      {usage && (
        <Card
          eyebrow={t("usage.totals.eyebrow")}
          title={t("usage.totals.title")}
        >
          <PeriodTotals label={t("usage.totals.today")} usage={usage.today} />
          <PeriodTotals label={t("usage.totals.month")} usage={usage.month} />
        </Card>
      )}

      {usage && usage.days.length > 0 && (
        <Card
          eyebrow={t("usage.history.eyebrow")}
          title={t("usage.history.title")}
        >
          <div className="ld-usage-table__scroll">
            <table className="ld-usage-table">
              <caption className="visually-hidden">
                {t("usage.history.caption")}
              </caption>
              <thead>
                <tr>
                  <th scope="col">{t("usage.history.day")}</th>
                  <th scope="col">{t("usage.history.incoming")}</th>
                  <th scope="col">{t("usage.history.outgoing")}</th>
                  <th scope="col">{t("usage.history.cost")}</th>
                </tr>
              </thead>
              <tbody>
                {usage.days.map((day) => (
                  <tr key={day.date}>
                    <th scope="row">
                      {format.date(localDay(day.date), {
                        weekday: "short",
                        day: "numeric",
                        month: "short",
                        year: "numeric",
                      })}
                    </th>
                    <td className="mono">
                      {t("usage.minutesShort", {
                        minutes: formatMinutes(format, day.incomingSeconds),
                      })}
                    </td>
                    <td className="mono">
                      {t("usage.minutesShort", {
                        minutes: formatMinutes(format, day.outgoingSeconds),
                      })}
                    </td>
                    <td className="mono">{formatCost(format, day.costUsd)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      )}

      {usage && usage.days.length === 0 && (
        <Card className="ld-empty">
          <span className="ld-empty__icon" aria-hidden="true">
            <ChartColumn size={24} />
          </span>
          <h2 className="ld-empty__title">{t("usage.emptyTitle")}</h2>
          <p className="ld-empty__text">{rich("usage.emptyText", { price })}</p>
        </Card>
      )}

      <SpendingCapCard />
    </>
  );
}

export default UsageScreen;
