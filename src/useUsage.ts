import { useEffect, useState } from "react";
import {
  getUsage,
  onSpendingCapReached,
  onUsageUpdated,
  type CapStatus,
  type UsageSnapshot,
} from "./lib/api";

export interface UsageState {
  /** `null` until loaded. */
  usage: UsageSnapshot | null;
  /** Why loading failed, if it did. */
  error: unknown;
  /** The cap that just stopped the translations (cleared once it no longer applies). */
  stoppedBy: CapStatus | null;
  /** Replaces the snapshot (after an action that returns a new one). */
  setUsage: (usage: UsageSnapshot) => void;
}

/** The core's usage snapshot, kept current through the `usage-updated` events. */
export function useUsage(): UsageState {
  const [usage, setUsage] = useState<UsageSnapshot | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [stoppedBy, setStoppedBy] = useState<CapStatus | null>(null);

  useEffect(() => {
    let current = true;
    const subscriptions = [
      onUsageUpdated((next) => {
        if (current) setUsage(next);
      }),
      onSpendingCapReached((status) => {
        if (current) setStoppedBy(status);
      }),
    ];
    getUsage()
      .then((loaded) => {
        // An event that arrived first is newer.
        if (current) setUsage((value) => value ?? loaded);
      })
      .catch((reason: unknown) => {
        if (current) setError(reason);
      });
    return () => {
      current = false;
      for (const subscription of subscriptions) {
        subscription.then((unlisten) => unlisten()).catch(() => undefined);
      }
    };
  }, []);

  // "Translation stopped" only while the cap still stops it.
  const capApplies =
    usage?.cap.level === "reached" && !usage.cap.overriddenToday;
  return {
    usage,
    error,
    stoppedBy: capApplies || usage === null ? stoppedBy : null,
    setUsage,
  };
}
