/**
 * Which kind of device this is (1.2.0-5). Features belong to a device, not
 * to a window size: keyboard shortcuts, windows, tray, push-to-talk keys
 * are for computers; widgets, fingerprint, glass panels, the floating
 * action bar are for phones. A narrow window on a PC is still a PC, and a
 * setting synced from the other kind of device must not switch on what
 * this one doesn't have.
 */
const UA = typeof navigator !== "undefined" ? navigator.userAgent : "";

export const IS_ANDROID = /android/i.test(UA);
export const IS_IOS = /iphone|ipad|ipod/i.test(UA);
/** A phone or tablet (the Android / iOS app). */
export const IS_MOBILE = IS_ANDROID || IS_IOS;
/** A computer (Windows, macOS, Linux). */
export const IS_DESKTOP = !IS_MOBILE;

/** A keyboard-shortcut hint for a tooltip — nothing on phones. */
export const kbd = (hint: string) => (IS_DESKTOP ? ` (${hint})` : "");
