import PageHeader from "../components/PageHeader";
import SettingsForm from "../SettingsForm";

function SettingsScreen() {
  return (
    <>
      <PageHeader
        title="Settings"
        description="Languages, devices and the app to translate, for each direction."
      />
      <SettingsForm />
    </>
  );
}

export default SettingsScreen;
