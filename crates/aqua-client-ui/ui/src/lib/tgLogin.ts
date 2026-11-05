/**
 * Confirming with Telegram (1.2.0-4, aqua-server/src/tglogin.rs): the
 * relay's bot opens in Telegram, the person taps Start, and the relay hands
 * back a one-time ticket — for sign-up, for sign-in on a new device (a
 * second factor) or for linking Telegram in the settings.
 */
import { api } from "./tauri";
import { tr } from "./i18n";

export interface TgConfirmed { ticket: string; name: string }

/** Open the bot and wait (up to 10 minutes) until Start is tapped.
 *  `signal.aborted` stops waiting (the window was closed). */
export async function confirmWithTelegram(serverUrl: string, signal?: { aborted: boolean }, onLink?: (link: string, bot: string) => void): Promise<TgConfirmed> {
  const s = await api.tgLoginStart(serverUrl);
  onLink?.(s.link, s.bot ?? "");
  try { await api.openExternal(s.link); } catch { window.open(s.link, "_blank"); }
  let errs = 0;
  const until = Date.now() + 10 * 60_000;
  while (Date.now() < until) {
    if (signal?.aborted) throw new Error("cancelled");
    await new Promise((r) => setTimeout(r, 2000));
    const p = await api.tgLoginPoll(serverUrl, s.code);
    if (p.done && p.ticket) return { ticket: p.ticket, name: p.name ?? "" };
    // The server can't read its bot (webhook, another program, bad token):
    // say why instead of waiting ten minutes for nothing.
    if (p.error) { if (++errs >= 2) throw new Error(explain(p.error)); } else errs = 0;
  }
  throw new Error("the Telegram sign-in has expired — start again");
}

const UNREACHABLE = "the server can't reach Telegram: ";
function explain(e: string): string {
  return e.startsWith(UNREACHABLE) ? tr(UNREACHABLE) + e.slice(UNREACHABLE.length) : tr(e);
}
