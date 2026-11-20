/**
 * The server's "over your share" answers (aqua-server quota.rs:
 * `quota: files <used> <limit>`, `quota: file_max <bytes>`, `quota: hist <bytes>`,
 * `quota: backup <bytes>`, `quota: bots <n>`) in people's words. Toast.svelte
 * passes every message through it, so each place that shows an error gets it.
 */
import { tr } from "./i18n";

function size(n: number): string {
  if (n >= 1073741824) return `${(n / 1073741824).toFixed(n % 1073741824 ? 1 : 0)} GB`;
  return `${Math.round(n / 1048576)} MB`;
}

export function quotaText(text: string): string | null {
  const m = /quota: (files|file_max|hist|backup|bots)((?: \d+)+)/.exec(text);
  if (!m) return null;
  const nums = m[2].trim().split(" ").map(Number);
  switch (m[1]) {
    case "files": return tr("Your space on the server is full ({used} of {limit}). Old files free it up, or a higher level gives more.", { used: size(nums[0]), limit: size(nums[1] ?? 0) });
    case "file_max": return tr("The file is too big for your level: up to {size}.", { size: size(nums[0]) });
    case "hist": return tr("History for your devices is full ({limit}): new messages stay on this device only.", { limit: size(nums[0]) });
    case "backup": return tr("The backup is bigger than your level allows ({limit}).", { limit: size(nums[0]) });
    case "bots": return nums[0] ? tr("Your level allows {n} bots.", { n: nums[0] }) : tr("Your level on this server doesn't include bots.");
  }
  return null;
}
