/**
 * Platform helper. Winthorpe ships macOS-only; retained as a single-source
 * helper so UI code can read "is this mac?" in one place instead of
 * hardcoding `true` everywhere. If Windows/Linux support is ever added
 * back, only this file changes.
 */

/** Always `true` — Winthorpe only runs on macOS. */
export function isMac(): boolean {
	return true;
}
