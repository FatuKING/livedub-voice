import type { SelectOption } from "../components/Select";
import type { Translate } from "../i18n/translate";
import type { AudioDevice } from "./api";

/** Select value for "use the system default device" (stored as `null` in settings). */
export const SYSTEM_DEFAULT_VALUE = "";

export interface DeviceOptionsInput {
  /** Devices from the core; `null` while they are loading or could not be listed. */
  devices: AudioDevice[] | null;
  kind: AudioDevice["kind"];
  /** Id stored in settings; `null` means the system default. */
  savedId: string | null;
  /** List virtual cables before other devices (for the outgoing output). */
  cablesFirst?: boolean;
  /** Translates the option labels. */
  t: Translate;
}

/**
 * Options for a device select: the system default, then the devices of `kind`.
 *
 * A saved id that is not in the list is kept as an extra option instead of being dropped, so
 * saving the form never loses it silently.
 */
export function deviceOptions({
  devices,
  kind,
  savedId,
  cablesFirst = false,
  t,
}: DeviceOptionsInput): SelectOption<string>[] {
  const ofKind = (devices ?? []).filter((device) => device.kind === kind);
  const ordered = cablesFirst
    ? [
        ...ofKind.filter((device) => device.isVirtualCable),
        ...ofKind.filter((device) => !device.isVirtualCable),
      ]
    : ofKind;

  const options: SelectOption<string>[] = [
    { value: SYSTEM_DEFAULT_VALUE, label: t("devices.systemDefault") },
    ...ordered.map((device) => ({
      value: device.id,
      label: device.isDefault
        ? t("devices.isDefault", { name: device.name })
        : device.name,
    })),
  ];

  if (savedId !== null && !ofKind.some((device) => device.id === savedId)) {
    options.push({
      value: savedId,
      label: devices === null ? t("devices.saved") : t("devices.unavailable"),
    });
  }
  return options;
}

/** Whether the list has a virtual cable of `kind`. */
export function hasVirtualCable(
  devices: AudioDevice[],
  kind: AudioDevice["kind"],
): boolean {
  return devices.some(
    (device) => device.kind === kind && device.isVirtualCable,
  );
}

/** Select value → settings value. */
export function deviceIdFromValue(value: string): string | null {
  return value === SYSTEM_DEFAULT_VALUE ? null : value;
}
