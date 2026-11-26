// Group video limits on phones (1.21.0-24): run with `npm run test:unit`.
import { groupVideoLimits } from "../src/lib/groupVideo";

const eq = (a: unknown, b: unknown, m: string) => { if (JSON.stringify(a) !== JSON.stringify(b)) { console.error("FAIL", m, a, b); process.exit(1); } };
const at = (copies: number, screen = false) => { const l = groupVideoLimits(copies, screen); return [l.maxBitrate / 1000, l.scaleResolutionDownBy]; };
eq(at(1), [1500, 1], "one other person");
eq(at(2), [700, 2], "two");
eq(at(3), [700, 2], "three");
eq(at(4), [350, 3], "four and more");
eq(at(8), [350, 3], "a full room");
eq(at(4, true), [350, 1], "a screen keeps its size");
// People leaving give the quality back.
eq(at(1), [1500, 1], "alone with one again");
console.log("group video: all ok");
