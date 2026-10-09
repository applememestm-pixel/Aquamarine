/**
 * UI language — English (source strings) or Russian.
 *
 * Usage in components: `$t("Settings")`, with `{name}` placeholders:
 * `$t("Left “{title}”", { title })`. Strings are keyed by their English
 * text, so a missing translation just shows English.
 */
import { writable, derived, get } from "svelte/store";
import { RU } from "./i18n.ru";

export type Lang = "en" | "ru";

function initial(): Lang {
  try {
    const v = localStorage.getItem("aqua.lang");
    if (v === "en" || v === "ru") return v;
  } catch { /* ignore */ }
  return typeof navigator !== "undefined" && /^ru|^uk|^be|^kk/i.test(navigator.language) ? "ru" : "en";
}

export const lang = writable<Lang>(initial());
lang.subscribe((v) => {
  try { localStorage.setItem("aqua.lang", v); } catch { /* ignore */ }
  if (typeof document !== "undefined") document.documentElement.lang = v;
});

type Vars = Record<string, string | number>;

function fill(s: string, vars?: Vars): string {
  if (!vars) return s;
  return s.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m));
}

export function translate(l: Lang, s: string, vars?: Vars): string {
  return fill(l === "ru" ? (RU[s] ?? s) : s, vars);
}

/** Reactive translator: `$t("Text")`. */
export const t = derived(lang, (l) => (s: string, vars?: Vars) => translate(l, s, vars));

/** Non-reactive, for plain .ts modules (toasts etc.). */
export function tr(s: string, vars?: Vars): string {
  return translate(get(lang), s, vars);
}

/** Count words: `$tn(3, "member")` → "3 members" / "3 участника". */
const PLURALS: Record<string, { en: [string, string]; ru: [string, string, string] }> = {
  member: { en: ["member", "members"], ru: ["участник", "участника", "участников"] },
  subscriber: { en: ["subscriber", "subscribers"], ru: ["подписчик", "подписчика", "подписчиков"] },
  vote: { en: ["vote", "votes"], ru: ["голос", "голоса", "голосов"] },
  rule: { en: ["rule", "rules"], ru: ["правило", "правила", "правил"] },
  chat: { en: ["chat", "chats"], ru: ["чат", "чата", "чатов"] },
};
function ruForm(n: number): 0 | 1 | 2 {
  const a = Math.abs(n) % 100, b = a % 10;
  if (a > 10 && a < 20) return 2;
  if (b === 1) return 0;
  if (b >= 2 && b <= 4) return 1;
  return 2;
}
export function plural(l: Lang, n: number, key: string): string {
  const p = PLURALS[key];
  if (!p) return `${n} ${key}`;
  return `${n} ${l === "ru" ? p.ru[ruForm(n)] : p.en[n === 1 ? 0 : 1]}`;
}
export const tn = derived(lang, (l) => (n: number, key: string) => plural(l, n, key));

export const LANGS: { id: Lang; name: string; native: string }[] = [
  { id: "en", name: "English", native: "English" },
  { id: "ru", name: "Russian", native: "Русский" },
];
