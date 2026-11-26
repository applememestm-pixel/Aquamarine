// Only changes cross over (1.21.0-24): run with `npm run test:unit`.
import { applyDelta, historyCache, type MessagesDelta } from "../src/lib/chatHistory";

const eq = (a: unknown, b: unknown, m: string) => { if (JSON.stringify(a) !== JSON.stringify(b)) { console.error("FAIL", m, a, b); process.exit(1); } };
const msg = (id: string, text = id) => ({ id, from: "me", time: "12:00", kind: { type: "text" as const, content: { text } } });
const d = (o: Partial<MessagesDelta>): MessagesDelta => ({ rev: 1, full: false, keep: 0, tail: [], changed: [], ...o });
const ids = (l: { id: string }[] | null) => l?.map((m) => m.id);

const base = [msg("1"), msg("2"), msg("3")];
eq(applyDelta(base, d({ keep: 3 })) === base, true, "nothing changed keeps the list");
const added = applyDelta(base, d({ keep: 3, tail: ["4"], changed: [msg("4")] }))!;
eq(ids(added), ["1", "2", "3", "4"], "appended");
eq(added[0] === base[0], true, "old objects stay");
const edited = applyDelta(base, d({ keep: 3, changed: [msg("2", "new")] }))!;
eq(edited[1].kind.content, { text: "new" }, "edited");
eq(ids(applyDelta(base, d({ keep: 1, tail: ["3"] }))), ["1", "3"], "removed");
eq(applyDelta(base, d({ keep: 5 })), null, "doesn't fit");
eq(applyDelta(base, d({ keep: 3, tail: ["9"] })), null, "unknown id");

const asked: (number | null)[] = [];
const answers = [
  d({ rev: 1, full: true, changed: base }),
  d({ rev: 2, keep: 3, tail: ["4"], changed: [msg("4")] }),
  d({ rev: 3, keep: 9 }),
  d({ rev: 4, full: true, changed: [msg("5")] }),
  d({ rev: 5, full: true, changed: base }),
];
const h = historyCache(async (_id, since) => { asked.push(since); return answers.shift()!; });
const [a, b] = await Promise.all([h.fetch("x"), h.fetch("x")]);
eq([ids(a), ids(b)], [["1", "2", "3"], ["1", "2", "3", "4"]], "one after another");
eq(ids(await h.fetch("x")), ["5"], "a bad delta is read anew");
h.forget();
await h.fetch("x");
eq(asked, [null, 1, 2, null, null], "what was asked");
console.log("history: all ok");
