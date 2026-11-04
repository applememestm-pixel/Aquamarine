/**
 * Confirming with Telegram (1.2.0-4, aqua-server/src/tglogin.rs): the
 * relay's bot opens in Telegram, the person taps Start, and the relay hands
 * back a one-time ticket — for sign-up, for sign-in on a new device (a
 * second factor) or for linking Telegram in the settings.
 */
import { api } from "./tauri";

export interface TgConfirmed { ticket: string; name: string }

/** Open the bot and wait (up to 10 minutes) until Start is tapped.
 *  `signal.aborted` stops waiting (the window was closed). */
export async function confirmWithTelegram(serverUrl: string, signal?: { aborted: boolean }): Promise<TgConfirmed> {
  const s = await api.tgLoginStart(serverUrl);
  try { await api.openExternal(s.link); } catch { window.open(s.link, "_blank"); }
  const until = Date.now() + 10 * 60_000;
  while (Date.now() < until) {
    if (signal?.aborted) throw new Error("cancelled");
    await new Promise((r) => setTimeout(r, 2000));
    const p = await api.tgLoginPoll(serverUrl, s.code);
    if (p.done && p.ticket) return { ticket: p.ticket, name: p.name ?? "" };
  }
  throw new Error("the Telegram sign-in has expired — start again");
}
