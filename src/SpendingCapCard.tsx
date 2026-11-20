import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import Button from "./components/Button";
import Card from "./components/Card";
import Notice from "./components/Notice";
import TextField from "./components/TextField";
import { useT } from "./i18n/context";
import { failureText, type Failure } from "./i18n/errors";
import { getSettings, saveSettings, type Settings } from "./lib/api";
import { OPENAI_LIMITS_URL, capText, parseCap } from "./lib/usage";
import "./SpendingCapCard.css";

/** Daily and monthly spending cap (in `settings.json`), plus a link to OpenAI's own budget. */
function SpendingCapCard() {
  const t = useT();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [daily, setDaily] = useState("");
  const [monthly, setMonthly] = useState("");
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState(false);
  const [invalid, setInvalid] = useState(false);
  const [error, setError] = useState<Failure | null>(null);

  useEffect(() => {
    let current = true;
    getSettings()
      .then((loaded) => {
        if (!current) return;
        setSettings(loaded);
        setDaily(capText(loaded.spendingCap.dailyUsd));
        setMonthly(capText(loaded.spendingCap.monthlyUsd));
      })
      .catch((reason: unknown) => {
        if (current) setError({ key: "usage.cap.loadError", reason });
      });
    return () => {
      current = false;
    };
  }, []);

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!settings) return;
    setSaved(false);
    setError(null);
    const dailyUsd = parseCap(daily);
    const monthlyUsd = parseCap(monthly);
    if (dailyUsd === undefined || monthlyUsd === undefined) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    setBusy(true);
    try {
      // Fresh settings, so changes made elsewhere since this card loaded are kept.
      const latest = await getSettings();
      const next = await saveSettings({
        ...latest,
        spendingCap: { dailyUsd, monthlyUsd },
      });
      setSettings(next);
      setDaily(capText(next.spendingCap.dailyUsd));
      setMonthly(capText(next.spendingCap.monthlyUsd));
      setSaved(true);
    } catch (reason: unknown) {
      setError({ key: "usage.cap.saveError", reason });
    } finally {
      setBusy(false);
    }
  }

  function openBudget() {
    openUrl(OPENAI_LIMITS_URL).catch((reason: unknown) =>
      setError({ key: "usage.cap.openError", reason }),
    );
  }

  return (
    <Card
      eyebrow={t("usage.cap.eyebrow")}
      title={t("usage.cap.title")}
      aria-label={t("usage.cap.ariaLabel")}
    >
      <p className="ld-cap-card__intro">{t("usage.cap.intro")}</p>
      <form
        className="ld-cap-card__form"
        onSubmit={(e) => void handleSubmit(e)}
      >
        <div className="ld-cap-card__fields">
          <TextField
            label={t("usage.cap.daily")}
            hint={t("usage.cap.dailyHint")}
            inputMode="decimal"
            placeholder={t("usage.cap.placeholder")}
            value={daily}
            disabled={!settings}
            aria-invalid={invalid && parseCap(daily) === undefined}
            onChange={(event) => {
              setDaily(event.target.value);
              setSaved(false);
            }}
          />
          <TextField
            label={t("usage.cap.monthly")}
            hint={t("usage.cap.monthlyHint")}
            inputMode="decimal"
            placeholder={t("usage.cap.placeholder")}
            value={monthly}
            disabled={!settings}
            aria-invalid={invalid && parseCap(monthly) === undefined}
            onChange={(event) => {
              setMonthly(event.target.value);
              setSaved(false);
            }}
          />
        </div>
        {invalid && <Notice tone="error">{t("usage.cap.invalid")}</Notice>}
        {saved && <Notice tone="success">{t("usage.cap.saved")}</Notice>}
        {error && <Notice tone="error">{failureText(error, t)}</Notice>}
        <div className="ld-cap-card__actions">
          <Button type="submit" disabled={!settings || busy}>
            {busy ? t("usage.cap.saving") : t("usage.cap.save")}
          </Button>
        </div>
      </form>
      <div className="ld-cap-card__openai">
        <Button
          variant="ghost"
          icon={<ExternalLink size={16} aria-hidden="true" />}
          onClick={openBudget}
        >
          {t("usage.cap.openaiBudget")}
        </Button>
        <p className="ld-cap-card__intro">{t("usage.cap.openaiHint")}</p>
      </div>
    </Card>
  );
}

export default SpendingCapCard;
