/**
 * Reactions use the "eltaller47" Telegram custom-emoji pack by default.
 *
 * A reaction is a string. Pack reactions are `ce:eltaller47/<file>|<emoji>`
 * — the part after `|` is the plain emoji, so someone without the pack
 * still sees a normal reaction. Plain reactions ("👍") keep working.
 *
 * Where the pack comes from, in order:
 *   1. bundled with the app: ui/public/reactions/eltaller47/ + manifest.json
 *      (tools/bundle-reactions.bat copies an imported pack there before a build)
 *   2. imported into the local sticker library (Telegram import)
 *   3. auto-import once at startup if a Telegram bot token is saved
 */
import { writable, get } from "svelte/store";
import { api, assetUrl } from "./tauri";
import type { StickerItem } from "./types";

export const REACTION_PACK = "eltaller47";
export const REACTION_PACK_LINK = "https://t.me/addemoji/eltaller47";
/** Telegram's default quick reactions — matched against the pack's emoji. */
export const DEFAULT_REACTIONS = ["👍", "❤️", "🔥", "😂", "😮", "😢", "🙏", "🎉"];

export interface ReactionItem {
  /** What gets sent / stored as the reaction. */
  token: string;
  url: string;
  mime: string;
  emoji: string;
  file: string;
}

export const reactionPack = writable<{ source: "bundled" | "imported" | "none"; items: ReactionItem[] }>({ source: "none", items: [] });

const strip = (e: string) => e.replace(/️/g, "");

/** Order a pack the way Telegram does: by the importer's running number. */
export function packOrder<T extends { path: string }>(items: T[]): T[] {
  const num = (p: string) => Number(/_(\d+)\.[a-z0-9]+$/i.exec(p)?.[1] ?? 1e9);
  return [...items].sort((a, b) => num(a.path) - num(b.path));
}

/** Emoji encoded in an imported file name ("👍_001.tgs" → "👍"). */
export function emojiFromFile(file: string): string {
  const i = file.indexOf("_");
  return i > 0 ? file.slice(0, i) : "";
}

export function parseReaction(r: string): { file: string | null; emoji: string; pack: string | null } {
  if (r.startsWith("ce:")) {
    const [ref, emoji] = r.slice(3).split("|");
    const pack = ref.split("/")[0] || null;
    const file = ref.split("/").slice(1).join("/") || null;
    return { file, pack, emoji: emoji || (file ? emojiFromFile(file) : "") || "★" };
  }
  return { file: null, pack: null, emoji: r };
}

/**
 * Every custom-emoji pack on this device, by Telegram short name — any of
 * them can be used for reactions (`ce:<pack>/<file>|<emoji>`).
 */
export interface EmojiPack { name: string; title: string; items: ReactionItem[] }
export const emojiPacks = writable<EmojiPack[]>([]);

export async function loadEmojiPacks(): Promise<void> {
  try {
    const lib = await api.listLocalMedia();
    emojiPacks.set(
      lib.packs
        .filter((p) => p.source && p.source !== REACTION_PACK && (p.kind === "emoji" || p.items.length))
        .map((p) => ({
          name: p.source!,
          title: p.title,
          items: packOrder(p.items).map((it) => {
            const file = it.path.split(/[\\/]/).pop() ?? "";
            const emoji = it.emoji ?? emojiFromFile(file);
            return { token: `ce:${p.source}/${file}|${emoji}`, url: assetUrl(it.path), mime: it.mime, emoji, file };
          }),
        })),
    );
  } catch { /* backend not ready */ }
}

/** Reactions from packs this device doesn't have: fetched from the relay once. */
const remote = new Map<string, { url: string; mime: string } | null>();
const MIME: Record<string, string> = { webp: "image/webp", png: "image/png", tgs: "application/x-tgsticker", webm: "video/webm", gif: "image/gif" };
function fetchRemote(pack: string, file: string) {
  const key = `${pack}/${file}`;
  if (remote.has(key)) return;
  remote.set(key, null);
  void api.packFilePath(pack, file).then((path) => {
    remote.set(key, { url: assetUrl(path), mime: MIME[file.split(".").pop() ?? ""] ?? "image/webp" });
    reactionPack.update((x) => ({ ...x })); // re-render reactions
  }).catch(() => { /* not on the relay: plain emoji fallback */ });
}

/** Where a reaction comes from (for "this reaction is from …"). */
export function reactionSource(r: string): { pack: string; title: string; local: boolean } | null {
  const { pack } = parseReaction(r);
  if (!pack) return null;
  if (pack === REACTION_PACK) return { pack, title: "El Taller", local: get(reactionPack).items.length > 0 };
  const p = get(emojiPacks).find((x) => x.name === pack);
  return { pack, title: p?.title ?? pack, local: !!p };
}

/** Image/animation URL for a reaction, or null if it's a plain emoji / unknown file. */
export function reactionUrl(r: string, items = get(reactionPack).items): { url: string; mime: string } | null {
  const { file, pack } = parseReaction(r);
  if (!file) return null;
  if (!pack || pack === REACTION_PACK) {
    const it = items.find((x) => x.file === file);
    if (it) return { url: it.url, mime: it.mime };
  }
  if (pack && pack !== REACTION_PACK) {
    const p = get(emojiPacks).find((x) => x.name === pack);
    const it = p?.items.find((x) => x.file === file);
    if (it) return { url: it.url, mime: it.mime };
  }
  if (!pack) return null;
  const hit = remote.get(`${pack}/${file}`);
  if (hit) return hit;
  fetchRemote(pack, file);
  return null;
}

/** The 8 reactions on the quick bar: Telegram's defaults, drawn from the pack. */
export function quickReactions(items = get(reactionPack).items): string[] {
  if (!items.length) return DEFAULT_REACTIONS;
  const out: string[] = [];
  for (const d of DEFAULT_REACTIONS) {
    const hit = items.find((i) => strip(i.emoji) === strip(d) && !out.includes(i.token));
    out.push(hit ? hit.token : d);
  }
  return out;
}

function fromLibraryItems(items: StickerItem[]): ReactionItem[] {
  return packOrder(items).map((it) => {
    const file = it.path.split(/[\\/]/).pop() ?? "";
    const emoji = it.emoji ?? emojiFromFile(file);
    return { token: `ce:${REACTION_PACK}/${file}|${emoji}`, url: assetUrl(it.path), mime: it.mime, emoji, file };
  });
}

const TRIED_KEY = "aqua.reactionPack.autoImportTried";

export async function loadReactionPack(): Promise<void> {
  void loadEmojiPacks();
  // 1. Bundled with the app build.
  try {
    const res = await fetch(`/reactions/${REACTION_PACK}/manifest.json`);
    if (res.ok) {
      const m = (await res.json()) as { files: { file: string; emoji: string; mime: string }[] };
      if (m.files?.length) {
        const items = packOrder(m.files.map((f) => ({ ...f, path: f.file }))).map((f) => ({
          token: `ce:${REACTION_PACK}/${f.file}|${f.emoji}`,
          url: `/reactions/${REACTION_PACK}/${encodeURIComponent(f.file)}`,
          mime: f.mime, emoji: f.emoji, file: f.file,
        }));
        reactionPack.set({ source: "bundled", items });
        return;
      }
    }
  } catch { /* not bundled */ }
  // 2. Imported into the local library.
  try {
    const lib = await api.listLocalMedia();
    const pack = lib.packs.find((p) => p.source === REACTION_PACK) ?? lib.packs.find((p) => /eltaller/i.test(p.title));
    if (pack) { reactionPack.set({ source: "imported", items: fromLibraryItems(pack.items) }); return; }
  } catch { /* backend not ready */ }
  reactionPack.set({ source: "none", items: [] });
  // 3. The relay hosts it (its admin set a Telegram token): fetch it from there.
  try {
    const srv = await api.serverPacks();
    if (srv?.packs?.some((p) => p.name === REACTION_PACK)) {
      await api.importPackViaServer(REACTION_PACK);
      const lib = await api.listLocalMedia();
      const pack = lib.packs.find((p) => p.source === REACTION_PACK);
      if (pack) { reactionPack.set({ source: "imported", items: fromLibraryItems(pack.items) }); return; }
    }
  } catch { /* not connected yet / no packs on the relay */ }
  // 4. One silent auto-import if the user already gave us a Telegram bot token.
  let token = "";
  try { token = localStorage.getItem("aqua.tgBotToken") ?? ""; } catch { /* ignore */ }
  let tried = false;
  try { tried = localStorage.getItem(TRIED_KEY) === "1"; } catch { /* ignore */ }
  if (token && !tried) {
    try { localStorage.setItem(TRIED_KEY, "1"); } catch { /* ignore */ }
    try { await importReactionPack(token); } catch (e) { console.warn("reaction pack auto-import", e); }
  }
}

/** Import the reaction pack from Telegram with a bot token. */
export async function importReactionPack(token: string): Promise<number> {
  const r = await api.importTelegramStickers(token, REACTION_PACK);
  try { localStorage.setItem("aqua.tgBotToken", token); } catch { /* ignore */ }
  await loadReactionPack();
  return r.imported;
}
