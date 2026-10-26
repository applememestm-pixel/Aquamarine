import { writable, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { tr } from "./i18n";

/**
 * In-app updates (Rust: updates.rs). Every v* tag is a release — stable,
 * or a numbered pre-release (1.0.61…) — and "every build" takes the newest
 * published one (the old moving "nightly" included); the app
 * compares itself with the newest one of its channel and, on a "yes",
 * downloads the file for this platform, checks its SHA-256 and installs it.
 */
declare const __AQ_BUILD__: { commit: string; channel: "nightly" | "stable" | "dev"; version: string };
export const BUILD = typeof __AQ_BUILD__ !== "undefined" ? __AQ_BUILD__ : { commit: "", channel: "dev" as const, version: "" };
export const BUILD_SHORT = BUILD.commit.slice(0, 7);

export type Channel = "nightly" | "stable";
interface Prefs { channel: Channel; auto: boolean; skip?: string }
const KEY = "aqua.updates.v1";
function loadPrefs(): Prefs {
  const def: Prefs = { channel: BUILD.channel === "stable" ? "stable" : "nightly", auto: true };
  try { return { ...def, ...JSON.parse(localStorage.getItem(KEY) || "{}") }; } catch { return def; }
}
export const updatePrefs = writable<Prefs>(loadPrefs());
updatePrefs.subscribe((p) => { try { localStorage.setItem(KEY, JSON.stringify(p)); } catch { /* storage blocked */ } });

export interface UpdateAsset { name: string; url: string; size: number; sha256: string | null }
export interface UpdateInfo {
  tag: string; name: string; commit: string | null; version: string | null;
  published_at: string; notes: string; html_url: string; asset: UpdateAsset | null;
}
export type UpdateState =
  | { s: "idle" } | { s: "checking" } | { s: "none"; info: UpdateInfo }
  | { s: "available"; info: UpdateInfo; id: string }
  | { s: "downloading"; info: UpdateInfo; id: string; done: number; total: number }
  | { s: "installing"; info: UpdateInfo; id: string }
  | { s: "permission"; info: UpdateInfo; id: string; path: string }
  | { s: "error"; error: string; info?: UpdateInfo; id?: string };
export const updateState = writable<UpdateState>({ s: "idle" });

/**
 * "1.0.10" > "1.0.9", and the project's pre-release numbering: after release
 * 1.0.6 come pre-releases 1.0.61, 1.0.62… and then release 1.0.7. So when
 * one patch number is longer, it's a pre-release of the shorter's line and
 * only its leading digits count against it: 1.0.7 > 1.0.62 > 1.0.61 > 1.0.6.
 */
export function newer(a: string, b: string): boolean {
  const sa = a.split(/[.-]/), sb = b.split(/[.-]/);
  const n = Math.max(sa.length, sb.length);
  for (let i = 0; i < n; i++) {
    let x = sa[i] ?? "0", y = sb[i] ?? "0";
    if (i === 2 && x.length !== y.length) {
      // 1.0.7 vs 1.0.62: compare 7 with 6; equal (1.0.6 vs 1.0.61) → the longer is newer.
      const short = Math.min(x.length, y.length);
      const hx = parseInt(x.slice(0, short), 10) || 0, hy = parseInt(y.slice(0, short), 10) || 0;
      if (hx !== hy) return hx > hy;
      return x.length > y.length;
    }
    const px = parseInt(x, 10) || 0, py = parseInt(y, 10) || 0;
    if (px !== py) return px > py;
  }
  return false;
}
/** The id of the release if it's not the build we are, else null. */
function updateId(info: UpdateInfo, channel: Channel): string | null {
  if (channel === "stable") return info.version && (!BUILD.version || newer(info.version, BUILD.version)) ? "v" + info.version : null;
  if (!info.commit) return null;
  return BUILD.commit.startsWith(info.commit) ? null : info.commit;
}

export async function checkUpdates(manual = false): Promise<void> {
  const cur = get(updateState).s;
  if (cur === "checking" || cur === "downloading" || cur === "installing") return;
  const { channel, skip } = get(updatePrefs);
  updateState.set({ s: "checking" });
  try {
    const info = await invoke<UpdateInfo>("update_check", { channel });
    const id = updateId(info, channel);
    // A build still uploading (no file or no checksum yet) waits for the next look.
    const ready = !!info.asset?.sha256;
    if (!id) updateState.set({ s: "none", info });
    else if (!ready) updateState.set(manual ? { s: "error", error: tr("The new build for this device isn't ready yet — try again in a few minutes."), info } : { s: "idle" });
    // A version the user put off stays quiet until they look themselves.
    else if (!manual && skip === id) updateState.set({ s: "idle" });
    else updateState.set({ s: "available", info, id });
  } catch (e) {
    updateState.set(manual ? { s: "error", error: String(e) } : { s: "idle" });
  }
}

export function skipUpdate() {
  const st = get(updateState);
  if ("id" in st && st.id) updatePrefs.update((p) => ({ ...p, skip: st.id }));
  updateState.set({ s: "idle" });
}

export async function installUpdate(): Promise<void> {
  const st = get(updateState);
  if (!("info" in st) || !st.info?.asset || !("id" in st) || !st.id) return;
  const { info, id } = st;
  const asset = info.asset!;
  let path = st.s === "permission" ? st.path : "";
  try {
    if (!path) {
      updateState.set({ s: "downloading", info, id, done: 0, total: asset.size });
      const un = await listen<{ done: number; total: number }>("aqua://update-progress", (e) =>
        updateState.update((x) => (x.s === "downloading" ? { ...x, done: e.payload.done, total: e.payload.total } : x)));
      try { path = await invoke<string>("update_download", { asset }); } finally { un(); }
    }
    updateState.set({ s: "installing", info, id });
    const r = await invoke<string>("update_install", { path });
    // Android: installs from the app need a one-time "allow" in system settings.
    if (r === "permission") updateState.set({ s: "permission", info, id, path });
  } catch (e) {
    updateState.set({ s: "error", error: String(e), info, id });
  }
}

/** Look on start and every 6 hours (GitHub allows 60 unauthenticated calls an hour). */
export function startUpdates(): () => void {
  const tick = () => { if (get(updatePrefs).auto && BUILD.channel !== "dev") void checkUpdates(); };
  const first = setTimeout(tick, 15000);
  const every = setInterval(tick, 6 * 3600 * 1000);
  // Back from "allow installs" in system settings: carry on.
  const onVis = () => { if (document.visibilityState === "visible" && get(updateState).s === "permission") void installUpdate(); };
  document.addEventListener("visibilitychange", onVis);
  return () => { clearTimeout(first); clearInterval(every); document.removeEventListener("visibilitychange", onVis); };
}
