/**
 * Who has stories right now (mine + the people whose presence carried
 * some), and the actions behind "Add a story".
 */
import { derived, get } from "svelte/store";
import { peerPresence } from "./presence";
import { myStories, seenStories, saneStories, storyClock, postStory, storiesOff, STORY_TTL_MS, type Story } from "./stories";
import { tr } from "./i18n";

export interface StoryOwner { user: string; stories: Story[]; unseen: boolean; last: number }

/** People with live stories, unseen first, newest first. Not me. */
export const storyOwners = derived([peerPresence, seenStories, storyClock, storiesOff], ([pres, seen, now, off]) => {
  const out: StoryOwner[] = [];
  if (off) return out;
  for (const [user, p] of Object.entries(pres)) {
    const stories = saneStories(p?.st, now);
    if (!stories.length) continue;
    out.push({ user, stories, unseen: stories.some((s) => !seen.includes(s.id)), last: Math.max(...stories.map((s) => s.at)) });
  }
  return out.sort((a, b) => Number(b.unseen) - Number(a.unseen) || b.last - a.last);
});

/** My live stories. */
export const myLiveStories = derived([myStories, storyClock, storiesOff], ([l, now, off]) => (off ? [] : l.filter((s) => now - s.at < STORY_TTL_MS)));

/** Switch stories off or on; my presence is sent again so mine go away / come back. */
export async function setStoriesOff(off: boolean) {
  storiesOff.set(off);
  const { broadcastPresence } = await import("./presence");
  broadcastPresence();
}

/** user → "unseen" | "seen" (a ring around their avatar), absent = none. */
export const storyRing = derived(storyOwners, (owners) => {
  const m: Record<string, "unseen" | "seen"> = {};
  for (const o of owners) m[o.user] = o.unseen ? "unseen" : "seen";
  return m;
});

/** "Add a story": the camera or a picked photo / video, then it's posted. */
export async function addStory(from: "camera" | "gallery") {
  let path: string | null = null;
  if (from === "camera") {
    const { openCamera } = await import("./components/CameraCapture.svelte");
    path = await openCamera();
  } else {
    try {
      const { openFileDialog } = await import("./dialog");
      const r = await openFileDialog({ multiple: false, directory: false, title: tr("Add a story"),
        filters: [{ name: tr("Photo or video"), extensions: ["jpg", "jpeg", "png", "webp", "gif", "mp4", "webm", "mov", "m4v"] }] });
      path = Array.isArray(r) ? (r[0] ?? null) : (r as string | null);
    } catch { path = null; }
  }
  if (!path) return;
  const { showToast } = await import("./components/Toast.svelte");
  showToast(tr("Posting the story…"), 30000);
  try {
    await postStory(String(path));
    showToast(tr("Story posted — it's visible for 24 hours"), 3500);
  } catch (e) {
    showToast(`${tr("Could not post the story")}: ${e}`, 6000);
  }
}

export function ownerList(): string[] {
  return ["", ...get(storyOwners).map((o) => o.user)].filter((u) => u !== "" || get(myLiveStories).length > 0);
}
