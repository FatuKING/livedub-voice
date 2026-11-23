import type { UiLanguage } from "../i18n/types";
import {
  getApiKeyStatus,
  getSettings,
  type ApiKeyStatus,
  type AudioDevice,
  type Settings,
} from "./api";

/** Where to create an OpenAI API key. */
export const OPENAI_API_KEYS_URL = "https://platform.openai.com/api-keys";

/** Where to download VB-CABLE, the free virtual cable the guide suggests. */
export const VB_CABLE_URL = "https://vb-audio.com/Cable/";

/**
 * Which first-run setup to show:
 * - `full`: every step (new users, or anyone who runs it again from Settings);
 * - `light`: privacy and the quick guide only, for existing users who already set up a key
 *   before the onboarding existed (they do not need the setup steps, but have never seen the
 *   consent note or the guide).
 */
export type OnboardingMode = "full" | "light";

/** A step of the first-run setup, in order. */
export type OnboardingStep =
  "welcome" | "apiKey" | "headphones" | "cable" | "privacy" | "guide";

export const ONBOARDING_STEPS: Record<OnboardingMode, OnboardingStep[]> = {
  full: ["welcome", "apiKey", "headphones", "cable", "privacy", "guide"],
  light: ["privacy", "guide"],
};

/** Which setup to show at launch, if any. */
export function onboardingMode(
  settings: Pick<Settings, "onboardingCompleted">,
  key: Pick<ApiKeyStatus, "hasKey">,
): OnboardingMode | null {
  if (settings.onboardingCompleted) return null;
  return key.hasKey ? "light" : "full";
}

/** What the panel needs to know before its first render. */
export interface Startup {
  uiLanguage: UiLanguage;
  onboarding: OnboardingMode | null;
}

/**
 * Reads the saved app language and whether to show the first-run setup. Without a core (plain
 * Vite preview) or with unreadable settings, the panel follows the OS and skips the setup; if
 * only the key cannot be read, the full setup is shown (its key step reports the problem).
 */
export async function loadStartup(): Promise<Startup> {
  let settings: Settings;
  try {
    settings = await getSettings();
  } catch {
    return { uiLanguage: "system", onboarding: null };
  }
  const key = await getApiKeyStatus().catch(() => ({ hasKey: false }));
  return {
    uiLanguage: settings.uiLanguage,
    onboarding: onboardingMode(settings, key),
  };
}

/** The virtual cables among `devices` that can be the outgoing output (their playback side). */
export function virtualCableOutputs(devices: AudioDevice[]): AudioDevice[] {
  return devices.filter(
    (device) => device.kind === "output" && device.isVirtualCable,
  );
}

/**
 * The cable to preselect as the outgoing output: the one already chosen if it is a cable, else
 * VB-CABLE's "CABLE Input" (what the guide describes), else the first one found.
 */
export function preferredCable(
  cables: AudioDevice[],
  currentId: string | null,
): AudioDevice | null {
  return (
    cables.find((cable) => cable.id === currentId) ??
    cables.find((cable) => /^cable input\b/i.test(cable.name)) ??
    cables[0] ??
    null
  );
}
