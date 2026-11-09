import ApiKeyCard from "../ApiKeyCard";
import PageHeader from "../components/PageHeader";
import SettingsForm from "../SettingsForm";

function SettingsScreen() {
  return (
    <>
      <PageHeader
        title="Settings"
        description="Your OpenAI API key, and the languages, devices and app to translate for each direction."
      />
      <ApiKeyCard />
      <SettingsForm />
    </>
  );
}

export default SettingsScreen;
