import { useCallback, useEffect, useMemo, useRef } from "react";
import type { EditorSessionState } from "@/lib/editor-session";
import { WorkspaceEditorSurface } from "./index";

/**
 * Renders the active file tab's editor surface. The tab BAR itself lives
 * one level up (src/features/editor/editor-tabs-bar.tsx) so it can stay
 * visible across both editor-tabs view AND conversation view — letting
 * users swap between the chat and any open file with a single click.
 *
 * Lifecycle of an open file:
 *   1. App calls `openFile(absolutePath)` (typically from FileTree click)
 *   2. If a tab for that path already exists → focus it
 *   3. Otherwise → push a new tab, mark active, surface lazy-loads content
 *   4. Editing toggles the tab's `dirty` flag automatically (via
 *      WorkspaceEditorSurface's onChangeSession callback)
 *   5. Save (Ctrl+S) is wired at the App level so it works regardless of
 *      whether the user is on the chat tab or a file tab.
 *   6. Closing a dirty tab prompts; closing the last tab returns to
 *      conversation view via `onAllClosed`
 */

export interface OpenTab {
	id: string;
	session: EditorSessionState;
}

interface TabbedEditorHostProps {
	tabs: OpenTab[];
	activeTabId: string | null;
	workspaceRootPath?: string | null;
	onTabsChange: (tabs: OpenTab[], activeTabId: string | null) => void;
	onAllClosed: () => void;
	onError?: (description: string, title?: string) => void;
}

export function TabbedEditorHost({
	tabs,
	activeTabId,
	workspaceRootPath,
	onTabsChange,
	onAllClosed,
	onError,
}: TabbedEditorHostProps) {
	const tabsRef = useRef(tabs);
	const activeIdRef = useRef(activeTabId);
	tabsRef.current = tabs;
	activeIdRef.current = activeTabId;

	const activeTab = useMemo(
		() => tabs.find((t) => t.id === activeTabId) ?? null,
		[tabs, activeTabId],
	);

	const updateActiveSession = useCallback(
		(session: EditorSessionState) => {
			const id = activeIdRef.current;
			if (!id) return;
			const next = tabsRef.current.map((t) =>
				t.id === id ? { ...t, session } : t,
			);
			onTabsChange(next, id);
		},
		[onTabsChange],
	);

	// Ctrl/Cmd+W to close the active tab. Save (Ctrl/Cmd+S) lives on the
	// App level so it works from any view (chat or editor) when there are
	// open file tabs.
	useEffect(() => {
		const handler = (event: KeyboardEvent) => {
			const cmd = event.ctrlKey || event.metaKey;
			if (!cmd) return;
			if (event.key === "w" || event.key === "W") {
				const id = activeIdRef.current;
				if (!id) return;
				const tab = tabsRef.current.find((t) => t.id === id);
				if (!tab) return;
				if (tab.session.dirty) {
					if (
						!window.confirm(
							"This file has unsaved changes. Close without saving?",
						)
					) {
						return;
					}
				}
				event.preventDefault();
				const remaining = tabsRef.current.filter((t) => t.id !== id);
				if (remaining.length === 0) {
					onTabsChange([], null);
					onAllClosed();
					return;
				}
				const closedIndex = tabsRef.current.findIndex((t) => t.id === id);
				const fallbackIndex = Math.max(0, closedIndex - 1);
				const nextActive =
					remaining[fallbackIndex]?.id ?? remaining[0]?.id ?? null;
				onTabsChange(remaining, nextActive);
			}
		};
		window.addEventListener("keydown", handler);
		return () => window.removeEventListener("keydown", handler);
	}, [onAllClosed, onTabsChange]);

	if (tabs.length === 0 || !activeTab) {
		// Caller should switch back to conversation view via onAllClosed
		// before this state is reachable; render nothing as a safety net.
		return null;
	}

	return (
		<section
			aria-label="File editor"
			data-focus-scope="editor"
			className="relative flex h-full min-h-0 flex-col overflow-hidden bg-background text-foreground"
		>
			{/* The editor surface — keyed by tab id so Monaco rebuilds the
			    model when the active tab changes. The surface itself caches
			    its model internally for fast switching. */}
			<div className="relative min-h-0 flex-1">
				<WorkspaceEditorSurface
					key={activeTab.id}
					editorSession={activeTab.session}
					workspaceRootPath={workspaceRootPath}
					onChangeSession={updateActiveSession}
					onExit={onAllClosed}
					onError={onError}
				/>
			</div>
		</section>
	);
}
