/**
 * The project's and the developer's places, in the order the owner wants
 * them: GitHub first, then Telegram, then X, then YouTube. Shown in
 * Settings → About and at the bottom of "What's new". An entry without a
 * url is left out.
 */
export interface Link { id: string; label: string; sub: string; url: string; icon: string }

const GH = '<path d="M12 2a10 10 0 0 0-3.2 19.5c.5.1.7-.2.7-.5v-1.7c-2.8.6-3.4-1.3-3.4-1.3-.5-1.2-1.1-1.5-1.1-1.5-.9-.6.1-.6.1-.6 1 .1 1.5 1 1.5 1 .9 1.5 2.4 1.1 2.9.8.1-.7.4-1.1.6-1.3-2.2-.3-4.6-1.1-4.6-5 0-1.1.4-2 1-2.7-.1-.3-.4-1.3.1-2.7 0 0 .8-.3 2.8 1a9.6 9.6 0 0 1 5 0c1.9-1.3 2.7-1 2.7-1 .6 1.4.2 2.4.1 2.7.7.7 1 1.6 1 2.7 0 3.9-2.3 4.7-4.6 5 .4.3.7.9.7 1.9v2.8c0 .3.2.6.7.5A10 10 0 0 0 12 2Z" fill="currentColor"/>';
const TG = '<path d="M21.9 4.3 18.6 20c-.2 1.1-.9 1.4-1.8.9l-5-3.7-2.4 2.3c-.3.3-.5.5-1 .5l.4-5.1 9.2-8.3c.4-.4-.1-.6-.6-.2L6 13.6 1.1 12c-1.1-.3-1.1-1.1.2-1.6L20.5 3c.9-.3 1.7.2 1.4 1.3Z" fill="currentColor"/>';
const X = '<path d="M17.8 3h3.1l-6.8 7.8 8 10.2h-6.3l-4.9-6.4L5.3 21H2.2l7.3-8.3L1.8 3h6.4l4.4 5.9L17.8 3Zm-1.1 16.2h1.7L7.2 4.7H5.4l11.3 14.5Z" fill="currentColor"/>';
const YT = '<path d="M23 7.2a3 3 0 0 0-2.1-2.1C19 4.6 12 4.6 12 4.6s-7 0-8.9.5A3 3 0 0 0 1 7.2 31 31 0 0 0 .5 12a31 31 0 0 0 .5 4.8 3 3 0 0 0 2.1 2.1c1.9.5 8.9.5 8.9.5s7 0 8.9-.5a3 3 0 0 0 2.1-2.1 31 31 0 0 0 .5-4.8 31 31 0 0 0-.5-4.8ZM9.8 15.1V8.9l5.8 3.1-5.8 3.1Z" fill="currentColor"/>';

export const LINKS: Link[] = ([
  { id: "gh", label: "GitHub", sub: "Aquamarine", url: "https://github.com/applememestm-pixel/Aquamarine", icon: GH },
  { id: "gh-dev", label: "GitHub", sub: "Developer", url: "https://github.com/vladstalker2027-stack", icon: GH },
  { id: "tg", label: "Telegram", sub: "Channel", url: "https://t.me/+IA-5SVnogZRhOTdi", icon: TG },
  { id: "tg-dev", label: "Telegram", sub: "Developer", url: "https://t.me/ShadowIn_TheNight", icon: TG },
  { id: "x", label: "X", sub: "Developer", url: "https://x.com/vpst27", icon: X },
  { id: "yt", label: "YouTube", sub: "Channel", url: "https://www.youtube.com/@VPST27", icon: YT },
] as Link[]).filter((l) => l.url);
