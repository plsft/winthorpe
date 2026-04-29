import type { OpenTab } from "./tabbed-editor-host";

/**
 * Per-workspace persistence for the editor's open-tabs state.
 *
 * Storage strategy:
 *   - localStorage, key = `winthorpe.editorTabs.<workspaceId>`
 *   - Persist only `id` + `path` per tab — `originalText` / `modifiedText`
 *     reload from disk on tab focus, so writing them would let stale
 *     in-memory edits silently overwrite filesystem changes made between
 *     app sessions.
 *   - Persist `activeTabId`.
 *   - `dirty` is implicitly false on rehydrate (mirrors VS Code: any
 *     unsaved buffer at quit is lost).
 *
 * Stale-path handling: `loadPersistedEditorTabs` takes a `verifyExists`
 * callback. We `Promise.all` it across every saved tab and silently
 * drop the ones whose file is gone. Callers should pass a wrapper around
 * `statEditorFile` that returns true only when `exists && isFile`.
 *
 * Best-effort: storage failures (quota, private mode) swallow silently.
 */

const STORAGE_KEY_PREFIX = "winthorpe.editorTabs.";
const STORAGE_VERSION = 1 as const;

interface PersistedTab {
	id: string;
	path: string;
}

interface PersistedSnapshot {
	version: typeof STORAGE_VERSION;
	tabs: PersistedTab[];
	activeTabId: string | null;
}

function storageKeyFor(workspaceId: string): string {
	return `${STORAGE_KEY_PREFIX}${workspaceId}`;
}

function readSnapshot(workspaceId: string): PersistedSnapshot | null {
	try {
		const raw = window.localStorage.getItem(storageKeyFor(workspaceId));
		if (!raw) return null;
		const parsed = JSON.parse(raw) as PersistedSnapshot;
		if (parsed?.version !== STORAGE_VERSION || !Array.isArray(parsed.tabs)) {
			return null;
		}
		return parsed;
	} catch {
		return null;
	}
}

function writeSnapshot(workspaceId: string, snapshot: PersistedSnapshot): void {
	try {
		window.localStorage.setItem(
			storageKeyFor(workspaceId),
			JSON.stringify(snapshot),
		);
	} catch {
		// Quota exceeded / private mode / etc. Silently swallow.
	}
}

function clearSnapshot(workspaceId: string): void {
	try {
		window.localStorage.removeItem(storageKeyFor(workspaceId));
	} catch {
		// ignore
	}
}

export interface LoadedTabs {
	tabs: OpenTab[];
	activeTabId: string | null;
}

/**
 * Load the persisted tab set for `workspaceId` and filter out tabs whose
 * file no longer exists on disk. Writes the cleaned-up snapshot back to
 * storage if anything was dropped, so subsequent loads don't re-stat dead
 * paths.
 */
export async function loadPersistedEditorTabs(
	workspaceId: string,
	verifyExists: (absolutePath: string) => Promise<boolean>,
): Promise<LoadedTabs> {
	const snapshot = readSnapshot(workspaceId);
	if (!snapshot || snapshot.tabs.length === 0) {
		return { tabs: [], activeTabId: null };
	}

	const checks = await Promise.all(
		snapshot.tabs.map(async (t) => ({
			tab: t,
			exists: await verifyExists(t.path).catch(() => false),
		})),
	);

	const surviving: OpenTab[] = checks
		.filter((c) => c.exists)
		.map((c) => ({
			id: c.tab.id,
			session: {
				kind: "file" as const,
				path: c.tab.path,
				dirty: false,
			},
		}));

	const survivingIds = new Set(surviving.map((t) => t.id));
	const restoredActive = snapshot.activeTabId
		? survivingIds.has(snapshot.activeTabId)
			? snapshot.activeTabId
			: (surviving[0]?.id ?? null)
		: (surviving[0]?.id ?? null);

	if (surviving.length !== snapshot.tabs.length) {
		writeSnapshot(workspaceId, {
			version: STORAGE_VERSION,
			tabs: surviving.map((t) => ({ id: t.id, path: t.session.path })),
			activeTabId: restoredActive,
		});
	}

	return { tabs: surviving, activeTabId: restoredActive };
}

/**
 * Persist `tabs` + `activeTabId` for `workspaceId`. Empty `tabs` clears
 * the storage entry rather than writing an empty snapshot.
 */
export function savePersistedEditorTabs(
	workspaceId: string,
	tabs: OpenTab[],
	activeTabId: string | null,
): void {
	if (tabs.length === 0) {
		clearSnapshot(workspaceId);
		return;
	}
	writeSnapshot(workspaceId, {
		version: STORAGE_VERSION,
		tabs: tabs.map((t) => ({ id: t.id, path: t.session.path })),
		activeTabId,
	});
}

/**
 * Wipe persisted state for `workspaceId`. Called by the workspace-delete
 * flow so dead workspaces don't leave dangling localStorage entries.
 */
export function clearPersistedEditorTabs(workspaceId: string): void {
	clearSnapshot(workspaceId);
}
