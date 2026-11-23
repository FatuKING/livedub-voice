import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ExternalLink,
  EyeOff,
  Headphones,
  Lock,
  Users,
  type LucideIcon,
} from "lucide-react";
import { useState } from "react";
import ApiKeyCard from "../ApiKeyCard";
import Button from "../components/Button";
import Card from "../components/Card";
import Notice from "../components/Notice";
import { useI18n, useT } from "../i18n/context";
import { failureText, type Failure } from "../i18n/errors";
import type { MessageKey } from "../i18n/types";
import { OPENAI_API_KEYS_URL } from "../lib/onboarding";
import { PRICE_PER_MINUTE_USD } from "../lib/pricing";
import { OPENAI_LIMITS_URL } from "../lib/usage";
import QuickGuide from "../QuickGuide";
import UiLanguageCard from "../UiLanguageCard";

/** Opens `url` in the browser; a failure is shown under `errorKey`. */
function useOpenUrl(errorKey: MessageKey) {
  const t = useT();
  const [error, setError] = useState<Failure | null>(null);
  function open(url: string) {
    setError(null);
    openUrl(url).catch((reason: unknown) =>
      setError({ key: errorKey, reason }),
    );
  }
  const notice = error && <Notice tone="error">{failureText(error, t)}</Notice>;
  return [open, notice] as const;
}

/** A browser link styled as a secondary button. */
export function LinkButton({
  label,
  onClick,
}: {
  label: string;
  onClick: () => void;
}) {
  return (
    <Button
      variant="secondary"
      icon={<ExternalLink size={16} aria-hidden="true" />}
      onClick={onClick}
    >
      {label}
    </Button>
  );
}

/** Step 1: what the app does, and the panel language (applied and saved at once). */
export function WelcomeStep() {
  const t = useT();
  return (
    <>
      <div className="ld-route-grid ld-route-grid--stretch">
        <Card direction="incoming" eyebrow={t("home.incoming.eyebrow")}>
          <p>{t("onboarding.welcome.incomingText")}</p>
        </Card>
        <Card direction="outgoing" eyebrow={t("home.outgoing.eyebrow")}>
          <p>{t("onboarding.welcome.outgoingText")}</p>
        </Card>
      </div>
      <UiLanguageCard />
    </>
  );
}

/** Step 2: where to get a key, what it costs, and the key itself (saved and checked for free). */
export function ApiKeyStep() {
  const { t, format } = useI18n();
  const [open, openError] = useOpenUrl("onboarding.apiKey.openError");
  return (
    <>
      <div className="ld-route-grid ld-route-grid--stretch">
        <Card
          eyebrow={t("onboarding.apiKey.howEyebrow")}
          title={t("onboarding.apiKey.howTitle")}
        >
          <ol className="ld-onboarding__list ld-onboarding__list--numbered">
            <li>{t("onboarding.apiKey.howStep1")}</li>
            <li>{t("onboarding.apiKey.howStep2")}</li>
            <li>{t("onboarding.apiKey.howStep3")}</li>
          </ol>
          <div className="ld-onboarding__buttons">
            <LinkButton
              label={t("onboarding.apiKey.openKeys")}
              onClick={() => open(OPENAI_API_KEYS_URL)}
            />
          </div>
        </Card>
        <Card
          eyebrow={t("onboarding.apiKey.costEyebrow")}
          title={t("onboarding.apiKey.costTitle")}
        >
          <p>
            {t("onboarding.apiKey.cost", {
              price: format.currency(PRICE_PER_MINUTE_USD, {
                maximumFractionDigits: 3,
              }),
              // One direction for an hour.
              hourly: format.currency(PRICE_PER_MINUTE_USD * 60, {
                maximumFractionDigits: 0,
              }),
            })}
          </p>
          <p className="ld-onboarding__muted">
            {t("onboarding.apiKey.budget")}
          </p>
          <div className="ld-onboarding__buttons">
            <LinkButton
              label={t("onboarding.apiKey.openBudget")}
              onClick={() => open(OPENAI_LIMITS_URL)}
            />
          </div>
        </Card>
      </div>
      {openError}
      <ApiKeyCard showIntro={false} />
    </>
  );
}

/** Step 3: why headphones. */
export function HeadphonesStep() {
  const t = useT();
  return (
    <Card
      eyebrow={t("onboarding.headphones.cardEyebrow")}
      title={t("onboarding.headphones.cardTitle")}
      aside={
        <span className="ld-onboarding__badge" aria-hidden="true">
          <Headphones size={20} />
        </span>
      }
    >
      <ul className="ld-onboarding__list">
        <li>{t("onboarding.headphones.point1")}</li>
        <li>{t("onboarding.headphones.point2")}</li>
      </ul>
    </Card>
  );
}

const PRIVACY_POINTS: {
  icon: LucideIcon;
  title: MessageKey;
  text: MessageKey;
}[] = [
  {
    icon: Users,
    title: "onboarding.privacy.othersTitle",
    text: "onboarding.privacy.others",
  },
  {
    icon: Lock,
    title: "onboarding.privacy.openaiTitle",
    text: "onboarding.privacy.openai",
  },
  {
    icon: EyeOff,
    title: "onboarding.privacy.telemetryTitle",
    text: "onboarding.privacy.telemetry",
  },
];

/** Step 5: what is sent where, and telling others about the translator. */
export function PrivacyStep({ light }: { light: boolean }) {
  const t = useT();
  return (
    <>
      {light && <Notice>{t("onboarding.privacy.lightIntro")}</Notice>}
      <Card>
        <ul className="ld-onboarding__points">
          {PRIVACY_POINTS.map(({ icon: Icon, title, text }) => (
            <li key={title} className="ld-onboarding__point">
              <span className="ld-onboarding__badge" aria-hidden="true">
                <Icon size={18} />
              </span>
              <div>
                <h2 className="ld-onboarding__point-title">{t(title)}</h2>
                <p className="ld-onboarding__muted">{t(text)}</p>
              </div>
            </li>
          ))}
        </ul>
      </Card>
    </>
  );
}

/** Step 6: the quick guide (also on Home). */
export function GuideStep() {
  return <QuickGuide />;
}
