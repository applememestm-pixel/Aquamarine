/**
 * Who has stories right now (mine + the people whose presence carried
 * some), and the actions behind "Add a story".
 */
import { derived, writable, get } from "svelte/store";
import { peerPresence } from "./presence";
import { myStories, seenStories, saneStories, storyClock, storiesOff, hiddenStories, isLive, type Story } from "./stories";

export interface StoryOwner { user: string; stories: Story[]; unseen: boolean; last: number; close: boolean; hidden: boolean }

/** Everyone with live stories, unseen first, newest first. Not me. */
const allOwners = derived([peerPresence, seenStories, storyClock, storiesOff, hiddenStories], ([pres, seen, now, off, hidden]) => {
  const out: StoryOwner[] = [];
  if (off) return out;
  for (const [user, p] of Object.entries(pres)) {
    const stories = saneStories(p?.st, now);
    if (!stories.length) continue;
    out.push({ user, stories, unseen: stories.some((s) => !seen.includes(s.id)), last: Math.max(...stories.map((s) => s.at)),
      close: stories.some((s) => s.cf), hidden: hidden.includes(user) });
  }
  return out.sort((a, b) => Number(b.unseen) - Number(a.unseen) || b.last - a.last);
});
/** The strip: people whose stories aren't hidden. */
export const storyOwners = derived(allOwners, (l) => l.filter((o) => !o.hidden));
/** Hidden ones (Telegram's "Hidden stories"): at the strip's end, dimmed. */
export const hiddenOwners = derived(allOwners, (l) => l.filter((o) => o.hidden));

/** My live stories. */
export const myLiveStories = derived([myStories, storyClock, storiesOff], ([l, now, off]) => (off ? [] : l.filter((s) => isLive(s, now))));

/** Switch stories off or on; my presence is sent again so mine go away / come back. */
export async function setStoriesOff(off: boolean) {
  storiesOff.set(off);
  const { broadcastPresence } = await import("./presence");
  broadcastPresence();
}

/** user → "unseen" | "seen" | "close" (a ring around their avatar), absent = none. */
export const storyRing = derived(storyOwners, (owners) => {
  const m: Record<string, "unseen" | "seen" | "close"> = {};
  for (const o of owners) m[o.user] = !o.unseen ? "seen" : o.close ? "close" : "unseen";
  return m;
});

/** A picked photo / video waiting in the story composer (StoryComposer). */
export const storyDraft = writable<{ path: string } | null>(null);

/** "Add a story": the camera or a picked photo / video, then the composer. */
export async function addStory(from: "camera" | "gallery") {
  let path: string | null = null;
  if (from === "camera") {
    const { openCamera } = await import("./components/CameraCapture.svelte");
    path = await openCamera();
  } else {
    try {
      const { tr } = await import("./i18n");
      const { openFileDialog } = await import("./dialog");
      const r = await openFileDialog({ multiple: false, directory: false, title: tr("Add a story"),
        filters: [{ name: tr("Photo or video"), extensions: ["jpg", "jpeg", "png", "webp", "gif", "mp4", "webm", "mov", "m4v"] }] });
      path = Array.isArray(r) ? (r[0] ?? null) : (r as string | null);
    } catch { path = null; }
  }
  if (path) storyDraft.set({ path: String(path) });
}

/** Everyone in the viewer's order: me (when I have some), then the strip. */
export function storyUsers(): string[] {
  return [...(get(myLiveStories).length ? [""] : []), ...get(storyOwners).map((o) => o.user)];
}
