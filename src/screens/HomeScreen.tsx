import { Play } from "lucide-react";
import { useEffect, useId, useState } from "react";
import Button from "../components/Button";
import Card from "../components/Card";
import LanguageRoute from "../components/LanguageRoute";
import Notice from "../components/Notice";
import PageHeader from "../components/PageHeader";
import Perforation from "../components/Perforation";
import StatusPill from "../components/StatusPill";
import {
  getSettings,
  isAppError,
  type Language,
  type Settings,
} from "../lib/api";
import {
  LANGUAGE_NAMES,
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

const COPY: Record<
  Direction,
  { eyebrow: string; title: (lang: string) => string; start: string }
> = {
  incoming: {
    eyebrow: "Incoming · others → you",
    title: (lang) => `Hear others in ${lang}`,
    start: "Start incoming",
  },
  outgoing: {
    eyebrow: "Outgoing · you → others",
    title: (lang) => `Others hear you in ${lang}`,
    start: "Start outgoing",
  },
};

function RouteCard({ direction, target, details }: RouteCardProps) {
  const noteId = useId();
  const copy = COPY[direction];
  return (
    <Card
      direction={direction}
      eyebrow={copy.eyebrow}
      title={copy.title(LANGUAGE_NAMES[target])}
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
            {copy.start}
          </Button>
          <StatusPill tone="sun">Coming soon</StatusPill>
        </div>
        <p className="ld-route-card__note" id={noteId}>
          Live translation is not available yet. It is coming in a next version.
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
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getSettings()
      .then(setSettings)
      .catch((reason: unknown) => {
        setError(isAppError(reason) ? reason.message : String(reason));
      });
  }, []);

  const incomingTarget =
    settings?.incoming.targetLanguage ?? FALLBACK_TARGETS.incoming;
  const outgoingTarget =
    settings?.outgoing.targetLanguage ?? FALLBACK_TARGETS.outgoing;
  const sourceApp =
    settings?.incoming.sourceApp.kind === "process"
      ? settings.incoming.sourceApp.name
      : "Whole system";

  return (
    <>
      <PageHeader
        title="Home"
        description="Two routes, one for each direction. Choose languages and devices in Settings."
      />
      {error && <Notice tone="error">Could not load settings: {error}</Notice>}
      <div className="ld-route-grid ld-route-grid--stretch">
        <RouteCard
          direction="incoming"
          target={incomingTarget}
          details={[
            { label: "Listens to", value: sourceApp },
            {
              label: "Plays on",
              value: settings?.incoming.outputDeviceId ?? "Default device",
            },
          ]}
        />
        <RouteCard
          direction="outgoing"
          target={outgoingTarget}
          details={[
            {
              label: "Listens to",
              value: settings?.outgoing.inputDeviceId ?? "Default microphone",
            },
            {
              label: "Sends to",
              value: settings?.outgoing.outputDeviceId ?? "Not set",
            },
          ]}
        />
      </div>
      <div>
        <Button variant="secondary" onClick={onOpenSettings}>
          Open settings
        </Button>
      </div>
    </>
  );
}

export default HomeScreen;
