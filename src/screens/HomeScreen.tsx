import { CircleHelp, X } from "lucide-react";
import { useEffect, useId, useState, type ReactNode } from "react";
import CapNotice from "../CapNotice";
import Button from "../components/Button";
import Card from "../components/Card";
import LanguageRoute from "../components/LanguageRoute";
import Notice from "../components/Notice";
import PageHeader from "../components/PageHeader";
import Perforation from "../components/Perforation";
import { useI18n } from "../i18n/context";
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
import { bindingOf, formatShortcut } from "../lib/hotkeys";
import { languageName, type Direction } from "../lib/languages";
import QuickGuide from "../QuickGuide";
import TranslationControls from "../TranslationControls";
import { useUsage } from "../useUsage";
import "./screens.css";

/** Original-audio volume shown before the saved settings load (the core's default). */
const DEFAULT_ORIGINAL_VOLUME = 20;

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
  const { t, locale } = useI18n();
  return (
    <Card
      direction={direction}
      eyebrow={t(`home.${direction}.eyebrow`)}
      title={t(`home.${direction}.title`, {
        language: languageName(target, locale),
      })}
    >
      <LanguageRoute direction={direction} to={target} />
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

/** What Home needs to know to enable the Start buttons. */
interface Readiness {
  /** `null` until known; the fake translator counts as a key. */
  hasKey: boolean | null;
  /** `null` until listed (or if listing failed). */
  devices: AudioDevice[] | null;
}

/** Why the outgoing translation cannot start, if it cannot. */
type OutgoingBlocker = "noApiKey" | "noOutput" | "outputMissing";

/** Why the incoming translation cannot start, if it cannot. */
type IncomingBlocker = "noApiKey" | "outputMissing";

/** Whether an output chosen in Settings is known to be gone. */
function outputMissing(id: string | null, devices: AudioDevice[] | null) {
  return (
    id !== null &&
    devices !== null &&
    !devices.some((d) => d.kind === "output" && d.id === id)
  );
}

function outgoingBlocker(
  settings: Settings | null,
  { hasKey, devices }: Readiness,
): OutgoingBlocker | null {
  if (hasKey === false) return "noApiKey";
  if (!settings) return null;
  const outputId = settings.outgoing.outputDeviceId;
  if (outputId === null) return "noOutput";
  if (outputMissing(outputId, devices)) return "outputMissing";
  return null;
}

function incomingBlocker(
  settings: Settings | null,
  { hasKey, devices }: Readiness,
): IncomingBlocker | null {
  if (hasKey === false) return "noApiKey";
  // No headphones chosen means the system default.
  if (settings && outputMissing(settings.incoming.outputDeviceId, devices))
    return "outputMissing";
  return null;
}

export interface HomeScreenProps {
  onOpenSettings: () => void;
  /** Opens the Usage screen (spending cap). */
  onOpenUsage?: () => void;
}

/** Overview of both translation routes, each with its controls. */
function HomeScreen({ onOpenSettings, onOpenUsage }: HomeScreenProps) {
  const { t, format } = useI18n();
  const { usage, stoppedBy, setUsage } = useUsage();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<Failure | null>(null);
  const [guideOpen, setGuideOpen] = useState(false);
  const guideId = useId();
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
  const outgoingBlocked = outgoingBlocker(settings, readiness);
  const incomingBlocked = incomingBlocker(settings, readiness);
  const originalVolume =
    settings?.incoming.originalVolume ?? DEFAULT_ORIGINAL_VOLUME;

  /** The toggle shortcut of `direction`, as a detail row (none when unset). */
  function shortcutDetail(direction: Direction) {
    const value = settings
      ? bindingOf(
          settings.hotkeys,
          direction === "incoming" ? "toggleIncoming" : "toggleOutgoing",
        )
      : null;
    return value
      ? [{ label: t("home.shortcut"), value: formatShortcut(value) }]
      : [];
  }

  const pushToTalkBinding =
    settings?.outgoing.talkMode === "pushToTalk"
      ? bindingOf(settings.hotkeys, "pushToTalk")
      : null;

  return (
    <>
      <PageHeader
        title={t("home.title")}
        description={t("home.description")}
        actions={
          <Button
            variant="secondary"
            icon={
              guideOpen ? (
                <X size={16} aria-hidden="true" />
              ) : (
                <CircleHelp size={16} aria-hidden="true" />
              )
            }
            aria-expanded={guideOpen}
            aria-controls={guideId}
            onClick={() => setGuideOpen((open) => !open)}
          >
            {guideOpen ? t("home.hideGuide") : t("home.howToUse")}
          </Button>
        }
      />
      <section
        id={guideId}
        className="ld-home-guide"
        aria-label={t("guide.title")}
        hidden={!guideOpen}
      >
        {guideOpen && <QuickGuide />}
      </section>
      {error && <Notice tone="error">{failureText(error, t)}</Notice>}
      {usage && (
        <CapNotice
          cap={usage.cap}
          stopped={stoppedBy !== null}
          onRaise={onOpenUsage}
          onContinued={setUsage}
        />
      )}
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
            ...shortcutDetail("incoming"),
          ]}
        >
          <TranslationControls
            direction="incoming"
            blocked={
              incomingBlocked && t(`home.incoming.blocked.${incomingBlocked}`)
            }
            help={t("home.incoming.help", {
              volume: format.percent(originalVolume / 100),
            })}
            onOpenSettings={onOpenSettings}
            onOpenUsage={onOpenUsage}
          />
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
            ...shortcutDetail("outgoing"),
          ]}
        >
          <TranslationControls
            direction="outgoing"
            blocked={
              outgoingBlocked && t(`home.outgoing.blocked.${outgoingBlocked}`)
            }
            warning={
              outgoingOutput?.isVirtualCable === false
                ? t("home.outgoing.notCable")
                : null
            }
            help={t("home.outgoing.help")}
            onOpenSettings={onOpenSettings}
            onOpenUsage={onOpenUsage}
            pushToTalkKey={
              pushToTalkBinding ? formatShortcut(pushToTalkBinding) : undefined
            }
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
