import { writable, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { tr } from "./i18n";
import { groupCall } from "./groupCall";

/**
 * In-app updates (Rust: updates.rs). Every v* tag is a release — stable,
 * or a numbered pre-release (1.20.1-6…). Releases are mandatory (the owner's
 * rule, 1.20.1-6): the app looks on start and every 3 hours, and a newer
 * release downloads, is checked against its SHA-256 and installs by itself —
 * there's no "Later". Testers (Settings → Updates → Tester) get every
 * numbered build the same way, pre-releases included. Nightly builds are gone.
 */
declare const __AQ_BUILD__: { commit: string; channel: "nightly" | "stable" | "dev"; version: string };
export const BUILD = typeof __AQ_BUILD__ !== "undefined" ? __AQ_BUILD__ : { commit: "", channel: "dev" as const, version: "" };
export const BUILD_SHORT = BUILD.commit.slice(0, 7);

export type Channel = "nightly" | "stable";
interface Prefs { tester: boolean }
const KEY = "aqua.updates.v2";
function loadPrefs(): Prefs {
  const def: Prefs = { tester: false };
  try { return { ...def, ...JSON.parse(localStorage.getItem(KEY) || "{}") }; } catch { return def; }
}
export const updatePrefs = writable<Prefs>(loadPrefs());
updatePrefs.subscribe((p) => { try { localStorage.setItem(KEY, JSON.stringify(p)); } catch { /* storage blocked */ } });
/** "nightly" is the Rust side's name for "every numbered build". */
const channelOf = (p: Prefs): Channel => (p.tester ? "nightly" : "stable");

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
  | { s: "signature"; info: UpdateInfo; id: string }
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
/** The id of the release if it's newer than the build we are, else null. */
function updateId(info: UpdateInfo): string | null {
  if (!info.version) return null;
  return !BUILD.version || newer(info.version, BUILD.version) ? "v" + info.version : null;
}

export async function checkUpdates(manual = false): Promise<void> {
  const cur = get(updateState).s;
  if (cur === "checking" || cur === "downloading" || cur === "installing") return;
  const channel = channelOf(get(updatePrefs));
  updateState.set({ s: "checking" });
  try {
    const info = await invoke<UpdateInfo>("update_check", { channel });
    const id = updateId(info);
    // A build still uploading (no file or no checksum yet) waits for the next look.
    const ready = !!info.asset?.sha256;
    if (!id) updateState.set({ s: "none", info });
    else if (!ready) updateState.set(manual ? { s: "error", error: tr("The new build for this device isn't ready yet — try again in a few minutes."), info } : { s: "idle" });
    else { updateState.set({ s: "available", info, id }); autoInstall(); }
  } catch (e) {
    updateState.set(manual ? { s: "error", error: String(e) } : { s: "idle" });
  }
}

/** Updates are mandatory: an available one starts by itself — not in the
 *  middle of a call (it waits for the call to end), and on Android the system
 *  installer is asked again at most every 30 minutes if it was turned down. */
let lastAuto = 0;
const busyWithCall = () => !!get(groupCall) || [...document.querySelectorAll<HTMLMediaElement>("[data-call-media]")].some((m) => !!m.srcObject);
function autoInstall() {
  const st = get(updateState);
  if (st.s !== "available" && st.s !== "error") return;
  if (!("id" in st) || !st.id) return;
  if (busyWithCall()) { setTimeout(autoInstall, 60000); return; }
  if (Date.now() - lastAuto < 30 * 60000) return;
  lastAuto = Date.now();
  void installUpdate();
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
    // Android: the new build is signed with another key — it can't go over
    // this one (the system would only say "App not installed").
    else if (r === "signature") updateState.set({ s: "signature", info, id });
  } catch (e) {
    updateState.set({ s: "error", error: String(e), info, id });
  }
}

/** Look on start and every 3 hours (GitHub allows 60 unauthenticated calls
 *  an hour; the site's updates.json takes over after that). */
export function startUpdates(): () => void {
  const tick = () => { if (BUILD.channel !== "dev") void checkUpdates(); };
  const first = setTimeout(tick, 15000);
  const every = setInterval(tick, 3 * 3600 * 1000);
  // Turning "Tester" on looks right away.
  let prevTester = get(updatePrefs).tester;
  const unPrefs = updatePrefs.subscribe((p) => { if (p.tester !== prevTester) { prevTester = p.tester; lastAuto = 0; void checkUpdates(true); } });
  // Back from "allow installs" in system settings: carry on.
  // Back from the system installer and still here: it didn't go through —
  // don't keep saying "Installing…" forever.
  let hiddenWhileInstalling = false;
  const onVis = () => {
    const s = get(updateState);
    if (document.visibilityState === "hidden") { hiddenWhileInstalling = s.s === "installing"; return; }
    if (s.s === "permission") void installUpdate();
    else if (s.s === "installing" && hiddenWhileInstalling) {
      hiddenWhileInstalling = false;
      setTimeout(() => {
        const n = get(updateState);
        if (n.s === "installing") { updateState.set({ s: "available", info: n.info, id: n.id }); autoInstall(); }
      }, 1500);
    } else if (s.s === "available") autoInstall();
  };
  document.addEventListener("visibilitychange", onVis);
  return () => { clearTimeout(first); clearInterval(every); unPrefs(); document.removeEventListener("visibilitychange", onVis); };
}
