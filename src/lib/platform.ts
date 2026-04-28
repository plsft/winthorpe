/**
 * Platform helpers — single source of truth for OS-conditional UI.
 *
 * Winthorpe is Windows-first, but the codebase ships cross-platform code
 * (the upstream Helmor source assumed macOS). UI code that needs to differ
 * between Windows/macOS reads from these helpers rather than hardcoding.
 */

import { type as osType } from "@tauri-apps/plugin-os";

let cachedKind: "macos" | "windows" | "linux" | "other" | null = null;

function detect(): "macos" | "windows" | "linux" | "other" {
	if (cachedKind) return cachedKind;
	// Tauri's os plugin gives us the host OS. Outside Tauri (Storybook,
	// Vitest, marketing site), fall back to navigator.userAgent so isMac()
	// still produces a sensible answer for tests that don't mock the plugin.
	try {
		const t = osType();
		const k =
			t === "macos" ? "macos" : t === "windows" ? "windows" : t === "linux" ? "linux" : "other";
		cachedKind = k;
		return k;
	} catch {
		const ua =
			typeof navigator !== "undefined" ? navigator.userAgent.toLowerCase() : "";
		const k =
			ua.includes("mac") ? "macos" :
			ua.includes("win") ? "windows" :
			ua.includes("linux") ? "linux" : "other";
		cachedKind = k;
		return k;
	}
}

export function isMac(): boolean {
	return detect() === "macos";
}

export function isWindows(): boolean {
	return detect() === "windows";
}

export function isLinux(): boolean {
	return detect() === "linux";
}

/** Reset the cache. Test-only — never call from production code. */
export function __resetPlatformCacheForTests(): void {
	cachedKind = null;
}
