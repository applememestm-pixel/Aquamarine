/**
 * Telegram-style compatibility: a message of a kind this build doesn't know
 * (a newer app sent it, `[something:v2]`) shows "update the app" instead of
 * raw JSON.
 */
const KNOWN = new Set([
  "article:v1", "board:v1", "capsule:v1", "cb:v1", "cmt:v1", "ctl:v1", "del:v1", "doc:v1", "edit:v1", "event:v1",
  "fwd:v1", "gcall:v1", "grpctl:v1", "grpinfo:v1", "grpjoin:v1", "grpleave:v1", "grpmsg:v1", "grpself:v1", "kb:v1",
  "poll:v1", "pollclose:v1", "pollvote:v1", "react:v1", "ren:v1", "reply:v1", "seen:v1", "status:v1", "sync:v1",
  "tasks:v1", "ttl:v1", "typing:v1", "wad:v1",
]);
const TAG = /^\[([a-z][a-z0-9_]{1,23}:v\d{1,3})\]\s*[{[]/;

/** The `kind:vN` of a structured message this build can't show, or null. */
export function unsupportedKind(text: string): string | null {
  const m = TAG.exec(text);
  return m && !KNOWN.has(m[1]) ? m[1] : null;
}
