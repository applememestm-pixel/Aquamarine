/**
 * A chat as a workspace: shared documents and boards next to the
 * checklists (planner.ts). Each starts as a chat message —
 *
 *   [doc:v1]{"id","title","body"}
 *   [board:v1]{"id","title","cols":["To do","Doing","Done"],"cards":[{"id","t","c"}]}
 *
 * — and changes travel as `[ctl:v1]` controls to everyone in the chat:
 *   {"t":"doc","doc","rev","body","title"?}        newest revision wins
 *   {"t":"card","board","id","text"?,"col"?,"del"?,"at"}   per card, newest wins
 *
 * State is kept per device (and in the encrypted backup).
 */
import { writable, get } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { sendChatControl } from "./tauri";
import { info } from "./stores";

export interface DocMsg { id: string; title: string; body: string }
export interface BoardCard { id: string; t: string; c: number }
export interface BoardMsg { id: string; title: string; cols: string[]; cards: BoardCard[] }

const clip = (s: unknown, n: number) => (typeof s === "string" ? s.slice(0, n) : "");

export function parseDoc(text: string): DocMsg | null {
  if (!text.startsWith("[doc:v1]")) return null;
  try {
    const v = JSON.parse(text.slice(8));
    if (typeof v?.id !== "string") return null;
    return { id: clip(v.id, 40), title: clip(v.title, 120) || "…", body: clip(v.body, 20000) };
  } catch { return null; }
}

export function parseBoard(text: string): BoardMsg | null {
  if (!text.startsWith("[board:v1]")) return null;
  try {
    const v = JSON.parse(text.slice(10));
    if (typeof v?.id !== "string") return null;
    const cols = (Array.isArray(v.cols) ? v.cols : []).filter((c: unknown) => typeof c === "string").slice(0, 8).map((c: string) => c.slice(0, 40));
    return {
      id: clip(v.id, 40), title: clip(v.title, 120) || "…", cols: cols.length ? cols : ["To do", "Doing", "Done"],
      cards: (Array.isArray(v.cards) ? v.cards : []).filter((c: BoardCard) => typeof c?.id === "string" && typeof c?.t === "string")
        .slice(0, 200).map((c: BoardCard) => ({ id: clip(c.id, 20), t: clip(c.t, 500), c: Math.max(0, Math.min(cols.length - 1, Number(c.c) || 0)) })),
    };
  } catch { return null; }
}

function persisted<T>(key: string, init: T) {
  let v = init;
  try { const raw = localStorage.getItem(key); if (raw) v = JSON.parse(raw); } catch { /* ignore */ }
  const s = writable<T>(v);
  s.subscribe((x) => { try { localStorage.setItem(key, JSON.stringify(x)); } catch { /* ignore */ } });
  return s;
}

export interface DocRev { rev: number; body: string; title?: string; by: string; at: number }
export const docState = persisted<Record<string, DocRev>>("aqua.docs.v1", {});
export interface CardState { t?: string; c?: number; del?: boolean; at: number; by: string }
export const boardState = persisted<Record<string, Record<string, CardState>>>("aqua.boards.v1", {});

/** The current text of a document (its newest revision, or the original). */
export function docNow(d: DocMsg): { title: string; body: string; rev: number; by?: string; at?: number } {
  const r = get(docState)[d.id];
  return r ? { title: r.title ?? d.title, body: r.body, rev: r.rev, by: r.by, at: r.at } : { title: d.title, body: d.body, rev: 0 };
}

function applyDoc(doc: string, r: DocRev) {
  docState.update((all) => {
    const cur = all[doc];
    // Newest revision wins; on a tie the later edit (then the name) decides.
    if (cur && (cur.rev > r.rev || (cur.rev === r.rev && (cur.at > r.at || (cur.at === r.at && cur.by >= r.by))))) return all;
    return { ...all, [doc]: r };
  });
}

export async function saveDoc(chat: string, d: DocMsg, title: string, body: string) {
  const me = get(info)?.username ?? "me";
  const rev = (docNow(d).rev || 0) + 1;
  const r: DocRev = { rev, body: body.slice(0, 20000), title: title.slice(0, 120), by: me, at: Date.now() };
  applyDoc(d.id, r);
  await sendChatControl(chat, "[ctl:v1]" + JSON.stringify({ t: "doc", doc: d.id, rev, body: r.body, title: r.title, at: r.at }));
}

/** Cards of a board as they are now. */
export function boardNow(b: BoardMsg, st: Record<string, Record<string, CardState>> = get(boardState)): BoardCard[] {
  const ops = st[b.id] ?? {};
  const out = new Map<string, BoardCard>();
  for (const c of b.cards) out.set(c.id, { ...c });
  for (const [id, o] of Object.entries(ops)) {
    if (o.del) { out.delete(id); continue; }
    const cur = out.get(id) ?? { id, t: "", c: 0 };
    out.set(id, { id, t: o.t ?? cur.t, c: Math.max(0, Math.min(b.cols.length - 1, o.c ?? cur.c)) });
  }
  return [...out.values()].filter((c) => c.t);
}

function applyCard(board: string, id: string, o: CardState) {
  boardState.update((all) => {
    const cur = all[board]?.[id];
    if (cur && cur.at > o.at) return all;
    return { ...all, [board]: { ...(all[board] ?? {}), [id]: { ...cur, ...o } } };
  });
}

export async function cardOp(chat: string, board: string, id: string, op: { text?: string; col?: number; del?: boolean }) {
  const me = get(info)?.username ?? "me";
  const o: CardState = { at: Date.now(), by: me, ...(op.text !== undefined ? { t: op.text.slice(0, 500) } : {}), ...(op.col !== undefined ? { c: op.col } : {}), ...(op.del ? { del: true } : {}) };
  applyCard(board, id, o);
  await sendChatControl(chat, "[ctl:v1]" + JSON.stringify({ t: "card", board, id, text: o.t, col: o.c, del: o.del, at: o.at }));
}

let started = false;
export function startWorkspace() {
  if (started) return;
  started = true;
  void listen<{ from: string; chat: string; body: Record<string, unknown>; own?: boolean }>("aqua://ctl", (e) => {
    const b = e.payload.body ?? {};
    const by = e.payload.own ? (get(info)?.username ?? "me") : e.payload.from;
    if (b.t === "doc") {
      const doc = clip(b.doc, 40);
      const rev = Number(b.rev);
      if (!doc || !Number.isFinite(rev) || rev < 1) return;
      applyDoc(doc, { rev, body: clip(b.body, 20000), title: clip(b.title, 120) || undefined, by, at: Number(b.at) || Date.now() });
    } else if (b.t === "card") {
      const board = clip(b.board, 40), id = clip(b.id, 20);
      if (!board || !id) return;
      const o: CardState = { at: Number(b.at) || Date.now(), by };
      if (typeof b.text === "string") o.t = b.text.slice(0, 500);
      if (typeof b.col === "number") o.c = b.col;
      if (b.del === true) o.del = true;
      applyCard(board, id, o);
    }
  }).catch(() => {});
}

export const newWsId = () => Date.now().toString(36) + Math.random().toString(36).slice(2, 7);
