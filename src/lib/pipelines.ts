import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  getIncomingStatus,
  getOutgoingStatus,
  onIncomingLevel,
  onIncomingStatus,
  onIncomingTranscript,
  onOutgoingLevel,
  onOutgoingStatus,
  onOutgoingTranscript,
  startIncoming,
  startOutgoing,
  stopIncoming,
  stopOutgoing,
  type InputLevel,
  type PipelineStatus,
} from "./api";
import type { Direction } from "./languages";
import type { PipelineState } from "./api";

/** Whether a direction holds its devices and a session (or is about to, or reconnects). */
export function isActive(state: PipelineState): boolean {
  return (
    state === "connecting" ||
    state === "live" ||
    state === "reconnecting" ||
    state === "stopping"
  );
}

/** The commands and events of one translation direction's pipeline. */
export interface PipelineApi {
  start: () => Promise<void>;
  stop: () => Promise<void>;
  getStatus: () => Promise<PipelineStatus>;
  onStatus: (handler: (status: PipelineStatus) => void) => Promise<UnlistenFn>;
  onLevel: (handler: (level: InputLevel) => void) => Promise<UnlistenFn>;
  onTranscript: (handler: (delta: string) => void) => Promise<UnlistenFn>;
}

/** Each direction's pipeline API. */
export const PIPELINES: Record<Direction, PipelineApi> = {
  incoming: {
    start: startIncoming,
    stop: stopIncoming,
    getStatus: getIncomingStatus,
    onStatus: onIncomingStatus,
    onLevel: onIncomingLevel,
    onTranscript: onIncomingTranscript,
  },
  outgoing: {
    start: startOutgoing,
    stop: stopOutgoing,
    getStatus: getOutgoingStatus,
    onStatus: onOutgoingStatus,
    onLevel: onOutgoingLevel,
    onTranscript: onOutgoingTranscript,
  },
};
