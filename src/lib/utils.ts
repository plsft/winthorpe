import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
	return twMerge(clsx(inputs));
}

/**
 * Extract a human-readable message from an unknown caught value.
 *
 * Handles all three error shapes the codebase encounters:
 *   1. Native `Error` instances (fetch/JSON/everything in JS land).
 *   2. Tauri command rejections — plain objects shaped `{ code, message }`
 *      from `CommandError`'s Serialize impl. The previous
 *      `error instanceof Error ? error.message : String(error)` pattern
 *      stringified these as the literal text "[object Object]".
 *   3. Strings, numbers, anything else — stringified.
 */
export function errorMessage(value: unknown): string {
	if (value == null) return "";
	if (typeof value === "string") return value;
	if (value instanceof Error) return value.message;
	if (typeof value === "object") {
		const maybeMessage = (value as { message?: unknown }).message;
		if (typeof maybeMessage === "string" && maybeMessage.length > 0) {
			return maybeMessage;
		}
	}
	return String(value);
}
