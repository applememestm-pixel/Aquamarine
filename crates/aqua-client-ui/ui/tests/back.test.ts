// The phone's Back button (lib/back.ts): the newest open thing closes first,
// one guard history entry while something is open, two presses to leave.
import { onBack, backOnce, startBack } from "../src/lib/back";

let failed = 0;
const check = (name: string, ok: boolean) => { if (!ok) { failed++; console.error("FAIL", name); } };

// A history with one entry per push; back() fires popstate.
const entries: unknown[] = [null];
const pops: Array<() => void> = [];
let pushes = 0;
const g = globalThis as unknown as Record<string, unknown>;
g.history = {
  get state() { return entries[entries.length - 1]; },
  pushState: (s: unknown) => { entries.push(s); pushes++; },
  back: () => { if (entries.length > 1) entries.pop(); pops.forEach((f) => f()); },
};
g.window = {
  addEventListener: (_: string, f: () => void) => { pops.push(f); },
  removeEventListener: () => {},
};
g.document = { body: { classList: { contains: () => false } }, querySelectorAll: () => [] };
g.MutationObserver = class { observe() {} disconnect() {} };

const stop = startBack();
const aquaBack = (g.window as { __aquaBack: () => boolean }).__aquaBack;
const closed: string[] = [];

const chat = onBack(() => closed.push("chat"));
onBack(() => closed.push("sheet"));
check("one guard entry for two open things", pushes === 1);
check("Back closes", aquaBack() === true);
check("the newest first", closed.join() === "sheet");
check("then the chat", aquaBack() === true && closed.join() === "sheet,chat");
chat(); // already closed by Back: nothing to do
check("releasing a closed one changes nothing", closed.length === 2 && entries.length === 2);

const menu = onBack(() => closed.push("menu"));
menu(); // closed with ×
check("the guard goes away with the last thing", entries.length === 1);
check("without closing it again", closed.length === 2);
check("nothing left to close", backOnce() === false);

check("the first Back on the main screen stays", aquaBack() === true);
check("the second leaves", aquaBack() === false);

// The WebView's own Back (older Android): popstate closes the top one and
// the guard comes back while something is still open.
onBack(() => closed.push("a"));
onBack(() => closed.push("b"));
(g.history as { back: () => void }).back();
check("popstate closes the newest", closed[closed.length - 1] === "b");
check("and keeps a guard for the rest", entries.length === 2);
stop();

if (failed) process.exit(1);
console.log("back: ok");
