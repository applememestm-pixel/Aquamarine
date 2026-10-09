/**
 * Global background audio player. One persistent <audio> element lives in
 * TopPlayer.svelte (mounted once in App), so playback keeps going when you
 * switch chats, tabs, or close the conversation you started it from —
 * music/podcasts don't stop just because the view changed.
 *
 * Any component calls `playTrack(...)` (single track) or `playQueue(...)`
 * (a list with prev/next); the top bar owns the element and all the
 * transport controls, and can expand into the full MusicPlayer sheet,
 * which drives the very same element.
 */

import { writable, get } from "svelte/store";

export interface Track {
  /** Media URL (data: / blob: / asset:). */
  src: string;
  /** Primary label (file name / track title). */
  title: string;
  /** Secondary label (chat name, size, …). */
  sub?: string;
  /** Optional cover art URL; a neon gradient tile is shown otherwise. */
  cover?: string;
}

/** Currently loaded track, or null when nothing is playing. */
export const nowPlaying = writable<Track | null>(null);

/** Current play queue (empty for a single `playTrack`). */
export const queue = writable<Track[]>([]);

/** Index of `nowPlaying` inside `queue` (-1 when not queued). */
export const queueIndex = writable<number>(-1);

/** Whether the full-size MusicPlayer sheet is open. */
export const playerExpanded = writable<boolean>(false);

/** Load a single track into the persistent top player and start it. */
export function playTrack(t: Track) {
  queue.set([]);
  queueIndex.set(-1);
  nowPlaying.set(t);
}

/** Play `tracks` as a queue, starting at `index`. */
export function playQueue(tracks: Track[], index = 0) {
  if (!tracks.length) return;
  const i = Math.max(0, Math.min(tracks.length - 1, index));
  queue.set(tracks.slice());
  queueIndex.set(i);
  nowPlaying.set(tracks[i]);
}

/** Advance to the next queued track. Returns false at the end of the queue. */
export function nextTrack(): boolean {
  const q = get(queue);
  const i = get(queueIndex);
  if (i < 0 || i + 1 >= q.length) return false;
  queueIndex.set(i + 1);
  nowPlaying.set(q[i + 1]);
  return true;
}

/** Go back to the previous queued track. Returns false at the start. */
export function prevTrack(): boolean {
  const q = get(queue);
  const i = get(queueIndex);
  if (i <= 0 || i >= q.length) return false;
  queueIndex.set(i - 1);
  nowPlaying.set(q[i - 1]);
  return true;
}

/** Open the full-size player sheet (no-op when nothing is loaded). */
export function expandPlayer() {
  if (get(nowPlaying)) playerExpanded.set(true);
}

/** Stop and hide the top player (and the expanded sheet). */
export function stopTrack() {
  playerExpanded.set(false);
  queue.set([]);
  queueIndex.set(-1);
  nowPlaying.set(null);
}
