import type { SerializedEditorState } from "lexical";

const STORAGE_PREFIX = "winthorpe:composer-draft:";

export function getComposerDraftStorageKey(contextKey: string): string {
	return `${STORAGE_PREFIX}${contextKey}`;
}

export function loadPersistedDraft(
	contextKey: string,
): SerializedEditorState | null {
	if (typeof window === "undefined") {
		return null;
	}

	try {
		const raw = window.localStorage.getItem(
			getComposerDraftStorageKey(contextKey),
		);
		if (!raw) {
			return null;
		}

		return JSON.parse(raw) as SerializedEditorState;
	} catch {
		return null;
	}
}

export function savePersistedDraft(
	contextKey: string,
	editorState: SerializedEditorState,
): void {
	if (typeof window === "undefined") {
		return;
	}

	const key = getComposerDraftStorageKey(contextKey);
	try {
		window.localStorage.setItem(key, JSON.stringify(editorState));
	} catch (error) {
		console.error(`[winthorpe] composer draft save failed for "${key}"`, error);
	}
}

export function clearPersistedDraft(contextKey: string): void {
	if (typeof window === "undefined") {
		return;
	}

	const key = getComposerDraftStorageKey(contextKey);
	try {
		window.localStorage.removeItem(key);
	} catch (error) {
		console.error(`[winthorpe] composer draft clear failed for "${key}"`, error);
	}
}
