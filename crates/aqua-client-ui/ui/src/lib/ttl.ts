/** Auto-delete timer presets and labels (autodelete.rs does the deleting). */
import { tr } from "./i18n";

export const TTL_PRESETS = [0, 300, 3600, 86400, 7 * 86400, 30 * 86400] as const;

/** "1 day", "5 min"… */
export function ttlLabel(secs: number): string {
  if (!secs) return tr("Off");
  if (secs % (30 * 86400) === 0) return tr("{n} mo", { n: secs / (30 * 86400) });
  if (secs % (7 * 86400) === 0) return tr("{n} wk", { n: secs / (7 * 86400) });
  if (secs % 86400 === 0) return tr("{n} d", { n: secs / 86400 });
  if (secs % 3600 === 0) return tr("{n} h", { n: secs / 3600 });
  if (secs % 60 === 0) return tr("{n} min", { n: secs / 60 });
  return tr("{n} s", { n: secs });
}

/** Short badge text: "5m", "1d", "1w". */
export function ttlShort(secs: number): string {
  if (secs % (30 * 86400) === 0) return `${secs / (30 * 86400)}mo`;
  if (secs % (7 * 86400) === 0) return `${secs / (7 * 86400)}w`;
  if (secs % 86400 === 0) return `${secs / 86400}d`;
  if (secs % 3600 === 0) return `${secs / 3600}h`;
  return `${Math.round(secs / 60)}m`;
}

const UNIT: Record<string, number> = { s: 1, min: 60, h: 3600, d: 86400, wk: 7 * 86400 };

/** The backend writes timer notes in English; show them in the UI language. */
export function ttlNote(text: string): string | null {
  let m = /^(You|@[a-z0-9_.]+) set messages to auto-delete after (\d+) (s|min|h|d|wk)$/.exec(text);
  if (m) {
    const lbl = ttlLabel(+m[2] * UNIT[m[3]]);
    return m[1] === "You" ? tr("You set messages to auto-delete after {t}", { t: lbl }) : tr("{who} set messages to auto-delete after {t}", { who: m[1], t: lbl });
  }
  m = /^(You|@[a-z0-9_.]+) turned off auto-delete$/.exec(text);
  if (m) return m[1] === "You" ? tr("You turned off auto-delete") : tr("{who} turned off auto-delete", { who: m[1] });
  return null;
}
