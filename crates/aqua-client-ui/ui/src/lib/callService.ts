// Android: while the microphone is in a call, a foreground service
// (AquaRelay.kt → AquaCallService) keeps it working with the app in the
// background. It can only start with the app on screen and the microphone
// allowed, so the wish is repeated until the call ends.
import type { Readable } from "svelte/store";

/** Is the microphone in use by a call: dialling, talking, or in a group call.
 *  A ringing incoming call is not. */
export function callServiceWanted(kind: string, groupJoined: boolean): boolean {
  return kind === "outgoing" || kind === "connected" || groupJoined;
}

export const CALL_SERVICE_RETRY_MS = 3000;

export function watchCallService(
  call: Readable<{ kind: string }>,
  group: Readable<{ joined: boolean } | null>,
  set: (on: boolean) => Promise<unknown>,
): () => void {
  let kind = "idle";
  let joined = false;
  let on = false;
  let timer: ReturnType<typeof setInterval> | undefined;
  // A refusal (no permission yet, app not on screen) is tried again.
  const ask = () => void set(true).catch(() => {});
  const apply = () => {
    const want = callServiceWanted(kind, joined);
    if (want === on) return;
    on = want;
    clearInterval(timer);
    if (want) {
      ask();
      timer = setInterval(ask, CALL_SERVICE_RETRY_MS);
    } else {
      void set(false).catch(() => {});
    }
  };
  const offCall = call.subscribe((s) => { kind = s.kind; apply(); });
  const offGroup = group.subscribe((g) => { joined = !!g?.joined; apply(); });
  return () => {
    offCall();
    offGroup();
    clearInterval(timer);
    if (on) { on = false; void set(false).catch(() => {}); }
  };
}

/** Phones: tell the native side while a call rings, for a notification. */
export function watchRinging(
  call: Readable<{ kind: string; peer?: string }>,
  ring: (peer: string, ringing: boolean) => Promise<unknown>,
): () => void {
  let peer: string | null = null;
  const set = (next: string | null) => {
    if (next === peer) return;
    if (peer !== null) void ring(peer, false).catch(() => {});
    peer = next;
    if (peer !== null) void ring(peer, true).catch(() => {});
  };
  const off = call.subscribe((s) => set(s.kind === "incoming" ? s.peer ?? null : null));
  return () => { off(); set(null); };
}
