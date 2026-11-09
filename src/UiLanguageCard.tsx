import { useRef, useState } from "react";
import Card from "./components/Card";
import Notice from "./components/Notice";
import Select from "./components/Select";
import { useI18n } from "./i18n/context";
import { failureText, type Failure } from "./i18n/errors";
import {
  LOCALES,
  LOCALE_NATIVE_NAMES,
  systemLanguages,
  systemLocale,
} from "./i18n/translate";
import type { UiLanguage } from "./i18n/types";
import { getSettings, saveSettings } from "./lib/api";
import "./UiLanguageCard.css";

/**
 * App language selector. A change applies to the whole panel at once and is saved right away,
 * on top of the settings the core holds (unsaved edits in the settings form are not touched).
 */
function UiLanguageCard() {
  const { t, preference, setPreference } = useI18n();
  const [error, setError] = useState<Failure | null>(null);
  // Saves run one after another so a quick second change never lands before the first.
  const queue = useRef<Promise<unknown>>(Promise.resolve());

  function change(next: UiLanguage) {
    setError(null);
    setPreference(next);
    const save = async () => {
      const current = await getSettings();
      await saveSettings({ ...current, uiLanguage: next });
    };
    queue.current = queue.current.then(save, save).catch((reason: unknown) => {
      setError({ key: "settings.uiLanguage.saveError", reason });
    });
  }

  const options: { value: UiLanguage; label: string }[] = [
    {
      value: "system",
      label: t("settings.uiLanguage.system", {
        language: LOCALE_NATIVE_NAMES[systemLocale(systemLanguages())],
      }),
    },
    ...LOCALES.map((locale) => ({
      value: locale,
      label: LOCALE_NATIVE_NAMES[locale],
    })),
  ];

  return (
    <Card className="ld-ui-language" eyebrow={t("settings.uiLanguage.eyebrow")}>
      <Select
        label={t("settings.uiLanguage.label")}
        hint={t("settings.uiLanguage.hint")}
        value={preference}
        options={options}
        onChange={change}
      />
      {error && <Notice tone="error">{failureText(error, t)}</Notice>}
    </Card>
  );
}

export default UiLanguageCard;
