// Pins per chat / room (1.21.0-23): run with `npm run test:unit` (esbuild + node).
const store: Record<string, string> = { "aqua-pins": JSON.stringify({ g1: "old", dm: "m1" }) };
(globalThis as any).localStorage = { getItem: (k: string) => store[k] ?? null, setItem: (k: string, v: string) => { store[k] = v; } };
const M = await import("../src/lib/messageMeta");
const eq = (a: unknown, b: unknown, m: string) => { if (JSON.stringify(a) !== JSON.stringify(b)) { console.error("FAIL", m, a, b); process.exit(1); } };
// Migration: old one-slot pins become lists.
eq(M.pinnedIds("dm"), ["m1"], "legacy dm");
// Rooms are independent; a room sees the chat's old slot too.
const A = M.pinScope("g1", "roomA"), B = M.pinScope("g1", "roomB");
M.pinMessage(A, "a1");
M.pinMessage(B, "b1");
eq(M.pinnedIds(A), ["a1", "old"], "room A keeps its own pin + legacy");
eq(M.pinnedIds(B), ["b1", "old"], "pinning in B did not replace A");
// Several pins, newest first, no duplicates, capped.
M.pinMessage(A, "a2"); M.pinMessage(A, "a1");
eq(M.pinnedIds(A).slice(0, 2), ["a1", "a2"], "re-pin moves to front");
for (let i = 0; i < 20; i++) M.pinMessage(A, "x" + i);
eq(JSON.parse(store["aqua-pins"])[A].length, M.MAX_PINS, "capped");
// Unpin one: only that one, from the room and the legacy slot.
M.unpinMessage(B, "old");
eq(M.pinnedIds(B), ["b1"], "unpinned legacy from B");
eq(M.isPinned(B, "b1"), true, "b1 still pinned");
M.unpinMessage(B, "b1");
eq(M.pinnedIds(B), [], "B empty");
eq(M.pinnedIds(A).includes("old"), false, "legacy gone for all rooms after unpin");
console.log("pins: all ok");
