/**
 * "Connections" in a profile (Discord-style): pick a service, type your
 * name there — the profile shows it with the service's colour and opens
 * the page. Stored as ordinary profile links ({label, url}); a link whose
 * address matches a service is shown as a connection.
 */
export interface Service {
  id: string;
  name: string;
  /** Brand-ish colour for the badge. */
  color: string;
  /** Page of `handle` there. */
  url: (handle: string) => string;
  /** Recognises the service's links and pulls the handle out. */
  re: RegExp;
  hint: string;
}

const h = (s: string) => encodeURIComponent(s.trim().replace(/^@/, ""));

export const SERVICES: Service[] = [
  { id: "twitch", name: "Twitch", color: "#9146FF", url: (x) => `https://twitch.tv/${h(x)}`, re: /^https?:\/\/(?:www\.)?twitch\.tv\/([^/?#]+)/i, hint: "twitch.tv/…" },
  { id: "x", name: "X", color: "#E7E9EA", url: (x) => `https://x.com/${h(x)}`, re: /^https?:\/\/(?:www\.)?(?:x|twitter)\.com\/([^/?#]+)/i, hint: "x.com/…" },
  { id: "pinterest", name: "Pinterest", color: "#E60023", url: (x) => `https://pinterest.com/${h(x)}`, re: /^https?:\/\/(?:[a-z]+\.)?pinterest\.[a-z.]+\/([^/?#]+)/i, hint: "pinterest.com/…" },
  { id: "youtube", name: "YouTube", color: "#FF0033", url: (x) => `https://youtube.com/@${h(x)}`, re: /^https?:\/\/(?:www\.)?youtube\.com\/@([^/?#]+)/i, hint: "youtube.com/@…" },
  { id: "github", name: "GitHub", color: "#C9D1D9", url: (x) => `https://github.com/${h(x)}`, re: /^https?:\/\/(?:www\.)?github\.com\/([^/?#]+)\/?$/i, hint: "github.com/…" },
  { id: "steam", name: "Steam", color: "#66C0F4", url: (x) => `https://steamcommunity.com/id/${h(x)}`, re: /^https?:\/\/steamcommunity\.com\/(?:id|profiles)\/([^/?#]+)/i, hint: "steamcommunity.com/id/…" },
  { id: "spotify", name: "Spotify", color: "#1ED760", url: (x) => `https://open.spotify.com/user/${h(x)}`, re: /^https?:\/\/open\.spotify\.com\/user\/([^/?#]+)/i, hint: "open.spotify.com/user/…" },
  { id: "instagram", name: "Instagram", color: "#E1306C", url: (x) => `https://instagram.com/${h(x)}`, re: /^https?:\/\/(?:www\.)?instagram\.com\/([^/?#]+)/i, hint: "instagram.com/…" },
  { id: "tiktok", name: "TikTok", color: "#25F4EE", url: (x) => `https://tiktok.com/@${h(x)}`, re: /^https?:\/\/(?:www\.)?tiktok\.com\/@([^/?#]+)/i, hint: "tiktok.com/@…" },
  { id: "reddit", name: "Reddit", color: "#FF4500", url: (x) => `https://reddit.com/user/${h(x)}`, re: /^https?:\/\/(?:www\.)?reddit\.com\/u(?:ser)?\/([^/?#]+)/i, hint: "reddit.com/user/…" },
  { id: "vk", name: "VK", color: "#0077FF", url: (x) => `https://vk.com/${h(x)}`, re: /^https?:\/\/(?:m\.)?vk\.com\/([^/?#]+)/i, hint: "vk.com/…" },
  { id: "telegram", name: "Telegram", color: "#2AABEE", url: (x) => `https://t.me/${h(x)}`, re: /^https?:\/\/t\.me\/([^/?#+]+)$/i, hint: "t.me/…" },
  { id: "soundcloud", name: "SoundCloud", color: "#FF5500", url: (x) => `https://soundcloud.com/${h(x)}`, re: /^https?:\/\/(?:www\.)?soundcloud\.com\/([^/?#]+)\/?$/i, hint: "soundcloud.com/…" },
  { id: "lastfm", name: "Last.fm", color: "#D51007", url: (x) => `https://last.fm/user/${h(x)}`, re: /^https?:\/\/(?:www\.)?last\.fm\/user\/([^/?#]+)/i, hint: "last.fm/user/…" },
  { id: "kick", name: "Kick", color: "#53FC18", url: (x) => `https://kick.com/${h(x)}`, re: /^https?:\/\/(?:www\.)?kick\.com\/([^/?#]+)/i, hint: "kick.com/…" },
];

/** The service a link belongs to, and the name on it. */
export function serviceOf(url: string): { s: Service; handle: string } | null {
  for (const s of SERVICES) {
    const m = s.re.exec(url.trim());
    if (m) { try { return { s, handle: decodeURIComponent(m[1]) }; } catch { return { s, handle: m[1] }; } }
  }
  return null;
}
