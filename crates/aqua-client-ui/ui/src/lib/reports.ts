/**
 * Error reports from the UI → src/crashreport.rs (the developer's report
 * service + the user's server). JS errors and unhandled promise failures;
 * no message text — the error, where it happened, the version.
 * Settings → About → "Send error reports" (on by default).
 */
import { get } from "svelte/store";
import { settings } from "./theme";
import { info } from "./stores";
import { codeOf } from "./errorCodes";

const inTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const call = (cmd: string, args: Record<string, unknown>) =>
  void import("@tauri-apps/api/core").then((c) => c.invoke(cmd, args)).catch(() => {});

export function reportError(kind: string, msg: string, detail = "") {
  if (!inTauri() || get(settings).reportErrors === false) return;
  // Noise, not bugs: the browser's own resize warnings, aborted loads.
  if (/ResizeObserver loop|AbortError|The user aborted|cancelled/i.test(msg)) return;
  call("report_error", { report: { kind, code: codeOf(msg), msg: msg.slice(0, 1000), detail: detail.slice(0, 6000) } });
}

export function startReports(): () => void {
  if (!inTauri()) return () => {};
  const onErr = (e: ErrorEvent) => {
    // Media elements fire plain "error" events: those are logged in main.ts.
    if (!(e instanceof ErrorEvent) || !e.message) return;
    reportError("js", e.message, `${e.filename ?? ""}:${e.lineno ?? 0}:${e.colno ?? 0}\n${e.error?.stack ?? ""}`);
  };
  const onRej = (e: PromiseRejectionEvent) => {
    const r = e.reason;
    reportError("js", r instanceof Error ? r.message : String(r), r instanceof Error ? r.stack ?? "" : "");
  };
  addEventListener("error", onErr);
  addEventListener("unhandledrejection", onRej);
  const push = () => call("report_config", { enabled: get(settings).reportErrors !== false, server: get(info)?.server_url ?? null });
  const a = settings.subscribe(push), b = info.subscribe(push);
  return () => { removeEventListener("error", onErr); removeEventListener("unhandledrejection", onRej); a(); b(); };
}
