import ApiKeyCard from "../ApiKeyCard";
import PageHeader from "../components/PageHeader";
import { useT } from "../i18n/context";
import SettingsForm from "../SettingsForm";
import UiLanguageCard from "../UiLanguageCard";

function SettingsScreen() {
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
    </>
  );
}

export default SettingsScreen;
