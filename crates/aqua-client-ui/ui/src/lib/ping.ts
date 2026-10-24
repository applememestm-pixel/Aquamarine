import { readable, get } from "svelte/store";
import { api } from "./tauri";
import { info } from "./stores";

/**
 * Round trip to the relay we're on, for the ping next to the title on the
 * main page (servers.rs `server_ping`). Measured every 15 s while the app is
 * in view and connected; null when there's nothing to show.
 */
export const ping = readable<number | null>(null, (set) => {
  let alive = true;
  let timer: ReturnType<typeof setTimeout> | null = null;
  const tick = async () => {
    timer = null;
    const i = get(info);
    if (!alive) return;
    if (i?.connected && i.server_url && document.visibilityState === "visible") {
      try { set(await api.serverPing(i.server_url)); } catch { set(null); }
    } else if (!i?.connected) set(null);
    if (alive) timer = setTimeout(tick, 15000);
  };
  // Measure right away when the connection comes up or moves to another relay.
  let was = "";
  const unsub = info.subscribe((i) => {
    const now = i?.connected ? i.server_url ?? "" : "";
    if (now === was) return;
    was = now;
    if (!now) set(null);
    else if (alive) { if (timer) clearTimeout(timer); timer = setTimeout(tick, 400); }
  });
  const onVis = () => { if (document.visibilityState === "visible" && alive) { if (timer) clearTimeout(timer); void tick(); } };
  document.addEventListener("visibilitychange", onVis);
  return () => { alive = false; if (timer) clearTimeout(timer); unsub(); document.removeEventListener("visibilitychange", onVis); };
});
