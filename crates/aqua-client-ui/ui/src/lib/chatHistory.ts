// The open chat is asked for again on every event. Only what changed since
// the last answer crosses over from the backend (history_delta.rs): the whole
// history was sent, parsed and compared for each new message.
import type { ChatMessage } from "./types";

export interface MessagesDelta {
  rev: number;
  /** `changed` is the whole chat. */
  full: boolean;
  /** How many messages of my copy, from the start, stay in place. */
  keep: number;
  /** The ids that follow them, in order. */
  tail: string[];
  changed: ChatMessage[];
}

/** The history after `d`; `base` itself when nothing changed, null when the
 *  delta doesn't fit it. */
export function applyDelta(base: ChatMessage[], d: MessagesDelta): ChatMessage[] | null {
  if (d.full) return d.changed;
  if (d.keep > base.length) return null;
  if (!d.tail.length && !d.changed.length && d.keep === base.length) return base;
  const byId = new Map(base.map((m) => [m.id, m]));
  for (const m of d.changed) byId.set(m.id, m);
  const out = base.slice(0, d.keep).map((m) => byId.get(m.id)!);
  for (const id of d.tail) {
    const m = byId.get(id);
    if (!m) return null;
    out.push(m);
  }
  return out;
}

/** Copies of the last few chats read, so opening one again is a delta too. */
export function historyCache(load: (id: string, since: number | null) => Promise<MessagesDelta>, size = 4) {
  const held = new Map<string, { rev: number; list: ChatMessage[] }>();
  /** One request per chat at a time: each builds on the one before. */
  const queue = new Map<string, Promise<unknown>>();
  let generation = 0;
  async function read(id: string): Promise<ChatMessage[]> {
    const started = generation;
    const base = held.get(id);
    let d = await load(id, base?.rev ?? null);
    let list = base || d.full ? applyDelta(base?.list ?? [], d) : null;
    if (!list) {
      d = await load(id, null);
      list = d.changed;
    }
    if (started !== generation) return read(id);
    held.delete(id);
    held.set(id, { rev: d.rev, list });
    if (held.size > size) held.delete(held.keys().next().value!);
    return list;
  }
  return {
    fetch(id: string): Promise<ChatMessage[]> {
      const next = (queue.get(id) ?? Promise.resolve()).catch(() => {}).then(() => read(id));
      queue.set(id, next);
      return next;
    },
    /** Read everything anew (a setting changed how messages are shown). */
    forget() { generation++; held.clear(); },
  };
}
