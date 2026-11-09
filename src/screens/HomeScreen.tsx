import { Play } from "lucide-react";
import { useEffect, useId, useState } from "react";
import Button from "../components/Button";
import Card from "../components/Card";
import LanguageRoute from "../components/LanguageRoute";
import Notice from "../components/Notice";
import PageHeader from "../components/PageHeader";
import Perforation from "../components/Perforation";
import StatusPill from "../components/StatusPill";
import { useT } from "../i18n/context";
import { failureText, type Failure } from "../i18n/errors";
import { getSettings, type Language, type Settings } from "../lib/api";
import {
  languageName,
  sourceLanguageFor,
  type Direction,
} from "../lib/languages";
import "./screens.css";

/** Languages shown before the saved settings load (or if they fail to load). */
const FALLBACK_TARGETS: Record<Direction, Language> = {
  incoming: "es",
  outgoing: "en",
};

interface RouteCardProps {
  direction: Direction;
  target: Language;
  details: { label: string; value: string }[];
}

function RouteCard({ direction, target, details }: RouteCardProps) {
  const t = useT();
  const noteId = useId();
  return (
    <Card
      direction={direction}
      eyebrow={t(`home.${direction}.eyebrow`)}
      title={t(`home.${direction}.title`, {
        language: languageName(target, t),
      })}
    >
      <LanguageRoute
        direction={direction}
        from={sourceLanguageFor(target)}
        to={target}
      />
      <Perforation />
      <dl className="ld-pass">
        {details.map(({ label, value }) => (
          <div key={label} className="ld-pass__item">
            <dt>{label}</dt>
            <dd title={value}>{value}</dd>
          </div>
        ))}
      </dl>
      <div className="ld-route-card__actions">
        <div className="ld-route-card__buttons">
          <Button
            variant={direction}
            icon={<Play size={16} aria-hidden="true" />}
            disabled
            aria-describedby={noteId}
          >
            {t(`home.${direction}.start`)}
          </Button>
          <StatusPill tone="sun">{t("home.comingSoon")}</StatusPill>
        </div>
        <p className="ld-route-card__note" id={noteId}>
          {t("home.comingSoonNote")}
        </p>
      </div>
    </Card>
  );
}

export interface HomeScreenProps {
  onOpenSettings: () => void;
}

/** Overview of both translation routes. Start/stop arrives with the audio pipelines. */
function HomeScreen({ onOpenSettings }: HomeScreenProps) {
  const t = useT();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<Failure | null>(null);

  useEffect(() => {
    getSettings()
      .then(setSettings)
      .catch((reason: unknown) => {
        setError({ key: "home.settingsError", reason });
      });
  }, []);

  const incomingTarget =
    settings?.incoming.targetLanguage ?? FALLBACK_TARGETS.incoming;
  const outgoingTarget =
    settings?.outgoing.targetLanguage ?? FALLBACK_TARGETS.outgoing;
  const sourceApp =
    settings?.incoming.sourceApp.kind === "process"
      ? settings.incoming.sourceApp.name
      : t("home.wholeSystem");

  return (
    <>
      <PageHeader title={t("home.title")} description={t("home.description")} />
      {error && <Notice tone="error">{failureText(error, t)}</Notice>}
      <div className="ld-route-grid ld-route-grid--stretch">
        <RouteCard
          direction="incoming"
          target={incomingTarget}
          details={[
            { label: t("home.listensTo"), value: sourceApp },
            {
              label: t("home.playsOn"),
              value:
                settings?.incoming.outputDeviceId ?? t("home.defaultDevice"),
            },
          ]}
        />
        <RouteCard
          direction="outgoing"
          target={outgoingTarget}
          details={[
            {
              label: t("home.listensTo"),
              value:
                settings?.outgoing.inputDeviceId ?? t("home.defaultMicrophone"),
            },
            {
              label: t("home.sendsTo"),
              value: settings?.outgoing.outputDeviceId ?? t("home.notSet"),
            },
          ]}
        />
      </div>
      <div>
        <Button variant="secondary" onClick={onOpenSettings}>
          {t("home.openSettings")}
        </Button>
      </div>
    </>
  );
}

export default HomeScreen;
