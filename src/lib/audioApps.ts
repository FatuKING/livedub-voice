import type { SelectOption } from "../components/Select";
import type { Translate } from "../i18n/translate";
import type { AudioApp } from "./api";

/** Select value for "no app chosen yet". */
export const NO_APP_VALUE = "";

/** Executable names are compared without case, as Windows does. */
export function sameProcess(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase();
}

/** `Discord.exe` → `Discord`. */
export function withoutExe(processName: string): string {
  return /^(.+)\.exe$/i.exec(processName)?.[1] ?? processName;
}

export interface AppOptionsInput {
  /** Apps from the core; `null` while they are loading or could not be listed. */
  apps: AudioApp[] | null;
  /** Executable name stored in settings; `null` when no app is chosen. */
  savedName: string | null;
  t: Translate;
}

/**
 * Options for the source app select: a "choose an app" placeholder while none is chosen, then the
 * apps (marking the ones playing now).
 *
 * A saved app that is not running is kept as an extra option instead of being dropped, so saving
 * the form never loses it silently.
 */
export function appOptions({
  apps,
  savedName,
  t,
}: AppOptionsInput): SelectOption<string>[] {
  const options: SelectOption<string>[] = [];
  if (savedName === null) {
    options.push({
      value: NO_APP_VALUE,
      label: t("settings.incoming.sourceAppChoose"),
    });
  }
  for (const app of apps ?? []) {
    options.push({
      value: app.processName,
      label: app.active
        ? t("settings.incoming.sourceAppPlaying", { name: app.displayName })
        : app.displayName,
    });
  }
  if (
    savedName !== null &&
    !(apps ?? []).some((app) => sameProcess(app.processName, savedName))
  ) {
    const name = withoutExe(savedName);
    options.push({
      value: savedName,
      label:
        apps === null
          ? name
          : t("settings.incoming.sourceAppNotRunning", { name }),
    });
  }
  return options;
}

/** The select value for the saved app (the listed app's own spelling when it is running). */
export function appSelectValue(
  apps: AudioApp[] | null,
  savedName: string | null,
): string {
  if (savedName === null) return NO_APP_VALUE;
  return (
    apps?.find((app) => sameProcess(app.processName, savedName))?.processName ??
    savedName
  );
}
