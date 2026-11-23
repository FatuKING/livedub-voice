import { openUrl } from "@tauri-apps/plugin-opener";
import { RefreshCw } from "lucide-react";
import { useEffect, useState } from "react";
import Button from "../components/Button";
import Card from "../components/Card";
import Notice from "../components/Notice";
import Select from "../components/Select";
import StatusPill from "../components/StatusPill";
import { useT } from "../i18n/context";
import { failureText, type Failure } from "../i18n/errors";
import {
  getSettings,
  listAudioDevices,
  saveSettings,
  type AudioDevice,
} from "../lib/api";
import {
  VB_CABLE_URL,
  preferredCable,
  virtualCableOutputs,
} from "../lib/onboarding";
import { LinkButton } from "./steps";

/** Makes `id` the outgoing output, on top of the latest saved settings. */
async function chooseOutgoingOutput(id: string) {
  const latest = await getSettings();
  if (latest.outgoing.outputDeviceId === id) return;
  await saveSettings({
    ...latest,
    outgoing: { ...latest.outgoing, outputDeviceId: id },
  });
}

/**
 * Step 4: the virtual cable for the outgoing translation. A cable found is chosen as the
 * outgoing output right away (the one already chosen, if it is a cable); without one, the step
 * explains how to install VB-CABLE and can look again.
 */
function CableStep() {
  const t = useT();
  /** Cables found; `null` while looking. */
  const [cables, setCables] = useState<AudioDevice[] | null>(null);
  const [chosenId, setChosenId] = useState<string | null>(null);
  const [chosen, setChosen] = useState(false);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<Failure | null>(null);
  /** Bumped by "Check again" to look once more. */
  const [attempt, setAttempt] = useState(0);

  async function choose(id: string) {
    setChosenId(id);
    setChosen(false);
    setError(null);
    try {
      await chooseOutgoingOutput(id);
      setChosen(true);
    } catch (reason: unknown) {
      setError({ key: "onboarding.cable.saveError", reason });
    }
  }

  useEffect(() => {
    let current = true;
    Promise.all([listAudioDevices(), getSettings()])
      .then(async ([devices, settings]) => {
        if (!current) return;
        const found = virtualCableOutputs(devices);
        const cable = preferredCable(found, settings.outgoing.outputDeviceId);
        setCables(found);
        if (!cable) return;
        setChosenId(cable.id);
        await chooseOutgoingOutput(cable.id).then(
          () => current && setChosen(true),
          (reason: unknown) =>
            current && setError({ key: "onboarding.cable.saveError", reason }),
        );
      })
      .catch((reason: unknown) => {
        if (!current) return;
        setCables((known) => known ?? []);
        setError({ key: "onboarding.cable.listError", reason });
      })
      .finally(() => {
        if (current) setBusy(false);
      });
    return () => {
      current = false;
    };
  }, [attempt]);

  function checkAgain() {
    setBusy(true);
    setError(null);
    setAttempt((value) => value + 1);
  }

  function download() {
    openUrl(VB_CABLE_URL).catch((reason: unknown) =>
      setError({ key: "onboarding.cable.openError", reason }),
    );
  }

  const errorNotice = error && (
    <Notice tone="error">{failureText(error, t)}</Notice>
  );

  if (cables === null) {
    return (
      <Card direction="outgoing" eyebrow={t("onboarding.cable.eyebrow")}>
        <p role="status" className="ld-onboarding__muted">
          {t("onboarding.cable.searching")}
        </p>
      </Card>
    );
  }

  const cable = cables.find((device) => device.id === chosenId);
  if (cable) {
    return (
      <Card
        direction="outgoing"
        eyebrow={t("onboarding.cable.eyebrow")}
        title={t("onboarding.cable.foundTitle")}
        aside={<StatusPill>{t("onboarding.cable.ready")}</StatusPill>}
      >
        <p>{t("onboarding.cable.foundText", { name: cable.name })}</p>
        {cables.length > 1 && (
          <Select
            label={t("onboarding.cable.chooseLabel")}
            value={cable.id}
            options={cables.map((device) => ({
              value: device.id,
              label: device.name,
            }))}
            onChange={(id) => void choose(id)}
          />
        )}
        {chosen && (
          <Notice tone="success">{t("onboarding.cable.chosen")}</Notice>
        )}
        {errorNotice}
      </Card>
    );
  }

  return (
    <Card
      direction="outgoing"
      eyebrow={t("onboarding.cable.eyebrow")}
      title={t("onboarding.cable.missingTitle")}
      aside={
        <StatusPill tone="sun">{t("onboarding.cable.missing")}</StatusPill>
      }
    >
      <p>{t("onboarding.cable.missingText")}</p>
      <div className="ld-onboarding__buttons">
        <LinkButton label={t("onboarding.cable.download")} onClick={download} />
        <Button
          variant="secondary"
          disabled={busy}
          icon={<RefreshCw size={16} aria-hidden="true" />}
          onClick={checkAgain}
        >
          {busy
            ? t("onboarding.cable.checking")
            : t("onboarding.cable.checkAgain")}
        </Button>
      </div>
      {errorNotice}
      <Notice>{t("onboarding.cable.incomingOnly")}</Notice>
    </Card>
  );
}

export default CableStep;
