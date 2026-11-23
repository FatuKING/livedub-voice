import { RotateCcw } from "lucide-react";
import ApiKeyCard from "../ApiKeyCard";
import Button from "../components/Button";
import Card from "../components/Card";
import PageHeader from "../components/PageHeader";
import { useT } from "../i18n/context";
import SettingsForm from "../SettingsForm";
import UiLanguageCard from "../UiLanguageCard";
import "./screens.css";

export interface SettingsScreenProps {
  /** Opens the first-run setup again. */
  onRunSetup?: () => void;
}

function SettingsScreen({ onRunSetup }: SettingsScreenProps) {
  const t = useT();
  return (
    <>
      <PageHeader
        title={t("settings.title")}
        description={t("settings.description")}
      />
      <UiLanguageCard />
      <ApiKeyCard />
      <SettingsForm />
      {onRunSetup && (
        <Card
          eyebrow={t("settings.setup.eyebrow")}
          title={t("settings.setup.title")}
          aria-label={t("settings.setup.ariaLabel")}
        >
          <p className="ld-setup-card__text">{t("settings.setup.text")}</p>
          <div>
            <Button
              variant="secondary"
              icon={<RotateCcw size={16} aria-hidden="true" />}
              onClick={onRunSetup}
            >
              {t("settings.setup.runAgain")}
            </Button>
          </div>
        </Card>
      )}
    </>
  );
}

export default SettingsScreen;
