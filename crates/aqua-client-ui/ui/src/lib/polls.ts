/**
 * Poll state. A poll message (MessageKind.Poll) carries a stable `id`;
 * votes travel as `[pollvote:v1]{poll, options, at}` control messages (1:1
 * over the session, groups via `group_control` fan-out). Each vote REPLACES
 * that voter's previous choice, so changing or retracting a vote (empty
 * `options`) is idempotent and tallies are exact for groups too. The poll's
 * author can close it (`[pollclose:v1]{poll}`).
 *
 * Kept per device in localStorage so results survive reloads.
 */

import { writable, get } from "svelte/store";

export const ME = "__me__";

export interface PollVote { o: number[]; at: number }
export interface PollState {
  /** voter → their current choice (ME = this device). */
  votes: Record<string, PollVote>;
  closed?: { by: string; at: number };
}
type States = Record<string, PollState>;

const KEY = "aqua-polls-v2";

function load(): States {
  try {
    const v2 = localStorage.getItem(KEY);
    if (v2) return JSON.parse(v2) as States;
    // Migrate the old count-only format.
    const old = JSON.parse(localStorage.getItem("aqua-polls") ?? "{}") as Record<string, { counts: Record<number, number>; mine: number[] }>;
    const out: States = {};
    for (const [id, t] of Object.entries(old)) {
      const votes: Record<string, PollVote> = {};
      if (t.mine?.length) votes[ME] = { o: [...t.mine], at: 0 };
      for (const [opt, n] of Object.entries(t.counts ?? {})) {
        const others = n - (t.mine?.includes(+opt) ? 1 : 0);
        for (let k = 0; k < others; k++) votes[`__old_${opt}_${k}`] = { o: [+opt], at: 0 };
      }
      out[id] = { votes };
    }
    return out;
  } catch {
    return {};
  }
}

export const pollStates = writable<States>(load());
pollStates.subscribe((t) => { try { localStorage.setItem(KEY, JSON.stringify(t)); } catch { /* ignore */ } });

function setVote(id: string, voter: string, o: number[], at = Date.now()) {
  pollStates.update((all) => {
    const cur = all[id] ?? { votes: {} };
    if (cur.closed) return all;
    const votes = { ...cur.votes };
    if (o.length) votes[voter] = { o: [...new Set(o)].sort((a, b) => a - b), at };
    else delete votes[voter];
    return { ...all, [id]: { ...cur, votes } };
  });
}

/** Our choice for poll `id`. Returns the new option list to broadcast,
 *  or null when nothing changed. */
export function castVote(id: string, option: number, multi: boolean): number[] | null {
  const st = get(pollStates)[id];
  if (st?.closed) return null;
  const cur = st?.votes[ME]?.o ?? [];
  let next: number[];
  if (multi) next = cur.includes(option) ? cur.filter((x) => x !== option) : [...cur, option];
  else {
    if (cur.length === 1 && cur[0] === option) return null;
    next = [option];
  }
  setVote(id, ME, next);
  return next;
}

export function retractVote(id: string): boolean {
  if (!get(pollStates)[id]?.votes[ME]) return false;
  setVote(id, ME, []);
  return true;
}

/** A vote from someone else. Accepts the new `{options}` and the old `{option}` shape. */
export function applyRemoteVote(from: string, v: { poll?: string; options?: unknown; option?: unknown; at?: unknown }) {
  if (!v || typeof v.poll !== "string") return;
  const at = typeof v.at === "number" ? v.at : Date.now();
  if (Array.isArray(v.options)) {
    setVote(v.poll, from, v.options.filter((x): x is number => Number.isInteger(x) && x >= 0 && x < 64), at);
  } else if (typeof v.option === "number") {
    const cur = get(pollStates)[v.poll]?.votes[from]?.o ?? [];
    setVote(v.poll, from, cur.includes(v.option) ? cur : [...cur, v.option], at);
  }
}

export function closePoll(id: string, by: string) {
  pollStates.update((all) => ({ ...all, [id]: { ...(all[id] ?? { votes: {} }), closed: { by, at: Date.now() } } }));
}

// ---- pure readers (pass the store value so templates stay reactive) ----

export interface PollView {
  counts: number[];
  voters: number;
  mine: number[];
  /** option → voters (for public polls). */
  who: { voter: string; at: number }[][];
  closed: boolean;
}

/**
 * `author` = who posted the poll; a close only counts if they sent it.
 * `closeAt` auto-closes on time.
 */
export function viewPoll(all: States, id: string, n: number, author: string, closeAt?: number | null): PollView {
  const st = all[id];
  const counts = new Array(n).fill(0) as number[];
  const who: { voter: string; at: number }[][] = Array.from({ length: n }, () => []);
  let voters = 0;
  for (const [voter, v] of Object.entries(st?.votes ?? {})) {
    let any = false;
    for (const o of v.o) if (o < n) { counts[o]++; who[o].push({ voter, at: v.at }); any = true; }
    if (any) voters++;
  }
  for (const w of who) w.sort((a, b) => b.at - a.at);
  const closed = (!!st?.closed && (st.closed.by === author || st.closed.by === ME)) || (!!closeAt && Date.now() >= closeAt);
  return { counts, voters, mine: st?.votes[ME]?.o ?? [], who, closed };
}
