import { ChartColumn } from "lucide-react";
import Card from "../components/Card";
import PageHeader from "../components/PageHeader";
import { useI18n } from "../i18n/context";
import { PRICE_PER_MINUTE_USD } from "../lib/pricing";
import "./screens.css";

/** Usage placeholder until local metering exists. */
function UsageScreen() {
  const { t, rich, format } = useI18n();
  return (
    <>
      <PageHeader
        title={t("usage.title")}
        description={t("usage.description")}
      />
      <Card className="ld-empty">
        <span className="ld-empty__icon" aria-hidden="true">
          <ChartColumn size={24} />
        </span>
        <h2 className="ld-empty__title">{t("usage.emptyTitle")}</h2>
        <p className="ld-empty__text">
          {rich("usage.emptyText", {
            price: (
              <span className="mono">
                {format.currency(PRICE_PER_MINUTE_USD, {
                  maximumFractionDigits: 3,
                })}
              </span>
            ),
          })}
        </p>
      </Card>
    </>
  );
}

export default UsageScreen;
