/** Badge glyphs for Discord-style server tags (24×24 stroke paths). */
export const CLAN_BADGES: Record<string, string> = {
  shield: `<path d="M12 3l7 3v5c0 5-3.5 8.5-7 10-3.5-1.5-7-5-7-10V6z"/>`,
  star: `<polygon points="12 3 14.8 9 21 9.6 16.3 13.8 17.8 20 12 16.7 6.2 20 7.7 13.8 3 9.6 9.2 9"/>`,
  bolt: `<polygon points="13 2 4 14 11 14 10 22 20 9 13 9 13 2"/>`,
  flame: `<path d="M12 22c4 0 7-3 7-7 0-5-5-7-5-12-3 2-6 6-6 9-1-1-2-2-2-4-2 2-2 4-2 6 0 5 4 8 8 8z"/>`,
  crown: `<path d="M3 8l4 4 5-7 5 7 4-4-2 11H5z"/>`,
  heart: `<path d="M20.8 4.6a5.5 5.5 0 0 0-7.8 0L12 5.7l-1-1.1a5.5 5.5 0 0 0-7.8 7.8L12 21l8.8-8.6a5.5 5.5 0 0 0 0-7.8z"/>`,
  diamond: `<polygon points="12 2 22 9 12 22 2 9"/>`,
  moon: `<path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/>`,
  leaf: `<path d="M4 20c10 0 16-6 16-16C10 4 4 10 4 20zM4 20l8-8"/>`,
  skull: `<path d="M12 3a8 8 0 0 0-8 8c0 3 1.5 5 4 6v3h8v-3c2.5-1 4-3 4-6a8 8 0 0 0-8-8z"/><circle cx="9" cy="11" r="1.5"/><circle cx="15" cy="11" r="1.5"/>`,
  music: `<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>`,
  game: `<rect x="2" y="7" width="20" height="11" rx="5"/><line x1="7" y1="11" x2="7" y2="14"/><line x1="5.5" y1="12.5" x2="8.5" y2="12.5"/><circle cx="16" cy="11.5" r="1"/><circle cx="18" cy="13.5" r="1"/>`,
  code: `<polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/>`,
  paw: `<circle cx="7" cy="9" r="2"/><circle cx="12" cy="6" r="2"/><circle cx="17" cy="9" r="2"/><path d="M12 12c-3 0-6 4-6 6 0 2 2 3 6 3s6-1 6-3c0-2-3-6-6-6z"/>`,
  sword: `<path d="M14.5 17.5L3 6V3h3l11.5 11.5"/><line x1="13" y1="19" x2="19" y2="13"/><line x1="16" y1="16" x2="20" y2="20"/>`,
  wave: `<path d="M2 12c3-4 5-4 8 0s5 4 8 0 3-2 4-2M2 18c3-4 5-4 8 0s5 4 8 0 3-2 4-2"/>`,
};
export const CLAN_COLORS = ["#00E5C7", "#7B5CFF", "#5865F2", "#FF7CAC", "#FFB067", "#2ECC71", "#FF4D6D", "#FFD93D", "#3E8EFF", "#B0B6C3"];

export function badgeSvg(id: string): string {
  const p = CLAN_BADGES[id] ?? CLAN_BADGES.shield;
  return `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">${p}</svg>`;
}
