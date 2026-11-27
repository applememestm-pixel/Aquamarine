// Full mesh: a device encodes its video once per other participant, so on a
// phone each copy gets smaller as the call grows.

/** `copies` is how many people the video goes to (not counting me). A shared
 *  screen keeps its resolution: it is mostly text. */
export function groupVideoLimits(copies: number, screen: boolean): { maxBitrate: number; scaleResolutionDownBy: number } {
  const step = copies >= 4 ? 2 : copies >= 2 ? 1 : 0;
  return { maxBitrate: [1500, 700, 350][step] * 1000, scaleResolutionDownBy: screen ? 1 : step + 1 };
}
