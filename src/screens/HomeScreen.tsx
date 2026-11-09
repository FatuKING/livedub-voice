import { Play } from "lucide-react";
import { useEffect, useId, useState, type ReactNode } from "react";
import Button from "../components/Button";
import Card from "../components/Card";
import LanguageRoute from "../components/LanguageRoute";
import Notice from "../components/Notice";
import PageHeader from "../components/PageHeader";
import Perforation from "../components/Perforation";
import StatusPill from "../components/StatusPill";
import { useT } from "../i18n/context";
import { failureText, type Failure } from "../i18n/errors";
import {
  getApiKeyStatus,
  getAppInfo,
  getSettings,
  listAudioDevices,
  type AudioDevice,
  type Language,
  type Settings,
} from "../lib/api";
import {
  languageName,
  sourceLanguageFor,
  type Direction,
} from "../lib/languages";
import OutgoingControls, { type OutgoingBlocker } from "../OutgoingControls";
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
  /** Controls at the bottom of the card. */
  children: ReactNode;
}

function RouteCard({ direction, target, details, children }: RouteCardProps) {
  const t = useT();
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
      <div className="ld-route-card__actions">{children}</div>
    </Card>
  );
}

/** The incoming direction is not wired yet. */
function ComingSoon() {
  const t = useT();
  const noteId = useId();
  return (
    <>
      <div className="ld-route-card__buttons">
        <Button
          variant="incoming"
          icon={<Play size={16} aria-hidden="true" />}
          disabled
          aria-describedby={noteId}
        >
          {t("home.incoming.start")}
        </Button>
        <StatusPill tone="sun">{t("home.comingSoon")}</StatusPill>
      </div>
      <p className="ld-route-card__note" id={noteId}>
        {t("home.comingSoonNote")}
      </p>
    </>
  );
}

/** What Home needs to know to enable the outgoing Start button. */
interface Readiness {
  /** `null` until known; the fake translator counts as a key. */
  hasKey: boolean | null;
  /** `null` until listed (or if listing failed). */
  devices: AudioDevice[] | null;
}

function outgoingBlocker(
  settings: Settings | null,
  { hasKey, devices }: Readiness,
): OutgoingBlocker | null {
  if (hasKey === false) return "noApiKey";
  if (!settings) return null;
  const outputId = settings.outgoing.outputDeviceId;
  if (outputId === null) return "noOutput";
  if (devices && !devices.some((d) => d.kind === "output" && d.id === outputId))
    return "outputMissing";
  return null;
}

export interface HomeScreenProps {
  onOpenSettings: () => void;
}

/** Overview of both translation routes, with the outgoing translation's controls. */
function HomeScreen({ onOpenSettings }: HomeScreenProps) {
  const t = useT();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<Failure | null>(null);
  const [readiness, setReadiness] = useState<Readiness>({
    hasKey: null,
    devices: null,
  });

  useEffect(() => {
    let current = true;
    getSettings()
      .then((loaded) => {
        if (current) setSettings(loaded);
      })
      .catch((reason: unknown) => {
        if (current) setError({ key: "home.settingsError", reason });
      });
    // A failed check leaves the value unknown: Start stays enabled and the core reports.
    Promise.all([getApiKeyStatus(), getAppInfo()])
      .then(([status, info]) => {
        if (!current) return;
        const hasKey = status.hasKey || info.fakeTranslator;
        setReadiness((value) => ({ ...value, hasKey }));
      })
      .catch(() => undefined);
    listAudioDevices()
      .then((devices) => {
        if (current) setReadiness((value) => ({ ...value, devices }));
      })
      .catch(() => undefined);
    return () => {
      current = false;
    };
  }, []);

  const incomingTarget =
    settings?.incoming.targetLanguage ?? FALLBACK_TARGETS.incoming;
  const outgoingTarget =
    settings?.outgoing.targetLanguage ?? FALLBACK_TARGETS.outgoing;
  const sourceApp =
    settings?.incoming.sourceApp.kind === "process"
      ? settings.incoming.sourceApp.name
      : t("home.wholeSystem");

  /** The device's name when it is connected, its id otherwise. */
  function deviceName(id: string): string {
    return readiness.devices?.find((device) => device.id === id)?.name ?? id;
  }

  const outgoingOutputId = settings?.outgoing.outputDeviceId ?? null;
  const outgoingOutput = readiness.devices?.find(
    (device) => device.kind === "output" && device.id === outgoingOutputId,
  );

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
              value: settings?.incoming.outputDeviceId
                ? deviceName(settings.incoming.outputDeviceId)
                : t("home.defaultDevice"),
            },
          ]}
        >
          <ComingSoon />
        </RouteCard>
        <RouteCard
          direction="outgoing"
          target={outgoingTarget}
          details={[
            {
              label: t("home.listensTo"),
              value: settings?.outgoing.inputDeviceId
                ? deviceName(settings.outgoing.inputDeviceId)
                : t("home.defaultMicrophone"),
            },
            {
              label: t("home.sendsTo"),
              value: outgoingOutputId
                ? deviceName(outgoingOutputId)
                : t("home.notSet"),
            },
          ]}
        >
          <OutgoingControls
            blocker={outgoingBlocker(settings, readiness)}
            outputIsCable={outgoingOutput?.isVirtualCable ?? true}
            onOpenSettings={onOpenSettings}
          />
        </RouteCard>
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
