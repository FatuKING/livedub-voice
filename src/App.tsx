import { useEffect, useState } from "react";
import SettingsForm from "./SettingsForm";
import { getAppInfo, isAppError, type AppInfo } from "./lib/api";

function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getAppInfo()
      .then(setInfo)
      .catch((reason: unknown) => {
        setError(isAppError(reason) ? reason.message : String(reason));
      });
  }, []);

  return (
    <main>
      <h1>LiveDub Voice</h1>
      {info && <p>Version {info.version}</p>}
      {error && <p role="alert">Could not read app info: {error}</p>}
      <p>LiveDub Voice is in early development.</p>
      <SettingsForm />
    </main>
  );
}

export default App;
