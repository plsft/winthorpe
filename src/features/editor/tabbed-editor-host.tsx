import { Save, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
	Tooltip,
	TooltipContent,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import { writeEditorFile } from "@/lib/api";
import {
	describeEditorPath,
	type EditorSessionState,
} from "@/lib/editor-session";
import { cn } from "@/lib/utils";
import { describeUnknownError } from "@/lib/workspace-helpers";
import { WorkspaceEditorSurface } from "./index";

/**
 * Multi-file tabbed editor.
 *
 * Owns a list of open files (each with its own EditorSessionState).
 * Renders a tab bar above the underlying WorkspaceEditorSurface and
 * switches the surface's session as the active tab changes.
 *
 * Lifecycle of an open file:
 *   1. App calls `openFile(absolutePath)` (typically from FileTree click)
 *   2. If a tab for that path already exists → focus it
 *   3. Otherwise → push a new tab, mark active, surface lazy-loads content
 *   4. Editing toggles the tab's `dirty` flag automatically (via
 *      WorkspaceEditorSurface's onChangeSession callback)
 *   5. Ctrl+S saves the active tab via `writeEditorFile` → clears dirty
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

	const [savingTabId, setSavingTabId] = useState<string | null>(null);

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

	const focusTab = useCallback(
		(id: string) => {
			if (id === activeTabId) return;
			onTabsChange(tabs, id);
		},
		[tabs, activeTabId, onTabsChange],
	);

	const closeTab = useCallback(
		(id: string) => {
			const tab = tabs.find((t) => t.id === id);
			if (!tab) return;
			if (tab.session.dirty) {
				const proceed = window.confirm(
					`${describeEditorPath(tab.session.path, workspaceRootPath)} has unsaved changes. Close without saving?`,
				);
				if (!proceed) return;
			}
			const remaining = tabs.filter((t) => t.id !== id);
			if (remaining.length === 0) {
				onTabsChange([], null);
				onAllClosed();
				return;
			}
			let nextActive = activeTabId;
			if (activeTabId === id) {
				// Focus the tab to the LEFT of the closed one (or the new
				// leftmost if we closed the leftmost).
				const closedIndex = tabs.findIndex((t) => t.id === id);
				const fallbackIndex = Math.max(0, closedIndex - 1);
				nextActive = remaining[fallbackIndex]?.id ?? remaining[0]?.id ?? null;
			}
			onTabsChange(remaining, nextActive);
		},
		[tabs, activeTabId, workspaceRootPath, onTabsChange, onAllClosed],
	);

	const saveActive = useCallback(async () => {
		const id = activeIdRef.current;
		if (!id) return;
		const tab = tabsRef.current.find((t) => t.id === id);
		if (!tab) return;
		const text = tab.session.modifiedText ?? "";
		if (!tab.session.dirty) return;

		setSavingTabId(id);
		try {
			await writeEditorFile(tab.session.path, text);
			// Clear dirty + advance originalText so further edits track from
			// the just-saved version.
			const next = tabsRef.current.map((t) =>
				t.id === id
					? {
							...t,
							session: {
								...t.session,
								originalText: text,
								dirty: false,
							},
						}
					: t,
			);
			onTabsChange(next, activeIdRef.current);
		} catch (error) {
			const message = describeUnknownError(error, "Failed to save the file.");
			onError?.(message, "Save failed");
		} finally {
			setSavingTabId(null);
		}
	}, [onError, onTabsChange]);

	// Ctrl/Cmd+S to save the active tab. Ctrl/Cmd+W to close the active tab.
	useEffect(() => {
		const handler = (event: KeyboardEvent) => {
			const cmd = event.ctrlKey || event.metaKey;
			if (!cmd) return;
			if (event.key === "s" || event.key === "S") {
				event.preventDefault();
				void saveActive();
				return;
			}
			if (event.key === "w" || event.key === "W") {
				const id = activeIdRef.current;
				if (!id) return;
				event.preventDefault();
				closeTab(id);
			}
		};
		window.addEventListener("keydown", handler);
		return () => window.removeEventListener("keydown", handler);
	}, [closeTab, saveActive]);

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
			{/* Tab bar */}
			<div
				className="flex h-9 shrink-0 items-end overflow-x-auto border-b border-border bg-background/60"
				role="tablist"
				aria-label="Open files"
			>
				{tabs.map((tab, index) => (
					<TabButton
						key={tab.id}
						tab={tab}
						index={index}
						active={tab.id === activeTabId}
						saving={savingTabId === tab.id}
						workspaceRootPath={workspaceRootPath ?? null}
						onFocus={() => focusTab(tab.id)}
						onClose={() => closeTab(tab.id)}
						onReorder={(fromId, toIndex) => {
							const fromIndex = tabs.findIndex((t) => t.id === fromId);
							if (fromIndex === -1 || fromIndex === toIndex) return;
							const next = [...tabs];
							const [moved] = next.splice(fromIndex, 1);
							if (!moved) return;
							// When dropping AFTER the original index we want toIndex
							// to refer to the position in the array AFTER removing the
							// dragged tab — splice already gave us that frame, so use
							// toIndex as-is when from < to (it shifts down by 1) and
							// toIndex when from > to (no shift).
							const insertAt = fromIndex < toIndex ? toIndex - 1 : toIndex;
							next.splice(insertAt, 0, moved);
							onTabsChange(next, activeTabId);
						}}
					/>
				))}
				<div className="flex flex-1 items-center justify-end gap-1 border-b border-border px-2">
					<Tooltip>
						<TooltipTrigger asChild>
							<Button
								type="button"
								variant="ghost"
								size="icon-xs"
								disabled={!activeTab.session.dirty || savingTabId !== null}
								onClick={() => void saveActive()}
								aria-label="Save (Ctrl+S)"
								className="text-muted-foreground hover:text-foreground"
							>
								<Save className="size-3.5" strokeWidth={1.7} />
							</Button>
						</TooltipTrigger>
						<TooltipContent
							side="bottom"
							className="rounded-md px-2 py-1 text-[11px]"
						>
							Save (Ctrl+S)
						</TooltipContent>
					</Tooltip>
				</div>
			</div>

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

interface TabButtonProps {
	tab: OpenTab;
	index: number;
	active: boolean;
	saving: boolean;
	workspaceRootPath: string | null;
	onFocus: () => void;
	onClose: () => void;
	onReorder: (fromTabId: string, toIndex: number) => void;
}

// MIME-style key for tab drag payload. Avoids colliding with text/plain
// (which triggers the OS's "drop file from desktop" path on some OSes).
const TAB_DND_TYPE = "application/x-winthorpe-tab";

function TabButton({
	tab,
	index,
	active,
	saving,
	workspaceRootPath,
	onFocus,
	onClose,
	onReorder,
}: TabButtonProps) {
	const display = useMemo(() => {
		const rel = describeEditorPath(tab.session.path, workspaceRootPath);
		// Show just the filename in the tab. Full relative path goes to
		// the title attribute for hover disambiguation.
		const slash = rel.lastIndexOf("/");
		return {
			label: slash === -1 ? rel : rel.slice(slash + 1),
			tooltip: rel,
		};
	}, [tab.session.path, workspaceRootPath]);

	// Drop indicator: which side (left|right) the cursor is hovering.
	// Updated on dragover so the user sees where the drop will land
	// before they release.
	const [dropSide, setDropSide] = useState<"left" | "right" | null>(null);
	const dragOverRef = useRef(false);

	const handleDragStart = useCallback(
		(event: React.DragEvent<HTMLDivElement>) => {
			event.dataTransfer.setData(TAB_DND_TYPE, tab.id);
			event.dataTransfer.effectAllowed = "move";
		},
		[tab.id],
	);

	const handleDragOver = useCallback(
		(event: React.DragEvent<HTMLDivElement>) => {
			// Only respond to our own tab drag — ignore file-from-desktop drops, etc.
			if (!event.dataTransfer.types.includes(TAB_DND_TYPE)) return;
			event.preventDefault();
			event.dataTransfer.dropEffect = "move";
			dragOverRef.current = true;
			const rect = event.currentTarget.getBoundingClientRect();
			const isLeftHalf = event.clientX - rect.left < rect.width / 2;
			setDropSide(isLeftHalf ? "left" : "right");
		},
		[],
	);

	const handleDragLeave = useCallback(() => {
		dragOverRef.current = false;
		// Small timeout so the indicator doesn't flicker as the cursor
		// moves between child elements.
		queueMicrotask(() => {
			if (!dragOverRef.current) setDropSide(null);
		});
	}, []);

	const handleDrop = useCallback(
		(event: React.DragEvent<HTMLDivElement>) => {
			const fromId = event.dataTransfer.getData(TAB_DND_TYPE);
			setDropSide(null);
			dragOverRef.current = false;
			if (!fromId || fromId === tab.id) return;
			event.preventDefault();
			// Drop on the LEFT half → insert at this index.
			// Drop on the RIGHT half → insert at this index + 1.
			const targetIndex = dropSide === "right" ? index + 1 : index;
			onReorder(fromId, targetIndex);
		},
		[dropSide, index, onReorder, tab.id],
	);

	return (
		<div
			role="tab"
			aria-selected={active}
			data-active={active || undefined}
			draggable
			onDragStart={handleDragStart}
			onDragOver={handleDragOver}
			onDragLeave={handleDragLeave}
			onDrop={handleDrop}
			className={cn(
				"group/tab relative flex h-full max-w-[200px] shrink-0 cursor-pointer items-center gap-1.5 border-r border-border px-3 text-[12px] transition-colors",
				active
					? "bg-background text-foreground"
					: "bg-background/30 text-muted-foreground hover:bg-foreground/5",
			)}
		>
			{/* Drop indicator. Pinned to the relevant edge of the tab so a
			    reorder lands visually where the user expects. */}
			{dropSide === "left" && (
				<span
					aria-hidden="true"
					className="pointer-events-none absolute left-0 top-0 z-20 h-full w-[2px] bg-foreground/80"
				/>
			)}
			{dropSide === "right" && (
				<span
					aria-hidden="true"
					className="pointer-events-none absolute right-0 top-0 z-20 h-full w-[2px] bg-foreground/80"
				/>
			)}
			<button
				type="button"
				onClick={onFocus}
				title={display.tooltip}
				className="flex min-w-0 flex-1 items-center gap-1.5 text-left cursor-pointer focus-visible:outline-none"
			>
				<span className="truncate">{display.label}</span>
				{tab.session.dirty && !saving && (
					<span
						aria-label="Unsaved changes"
						className="size-1.5 shrink-0 rounded-full bg-foreground/60"
					/>
				)}
				{saving && (
					<span
						aria-label="Saving"
						className="size-1.5 shrink-0 animate-pulse rounded-full bg-blue-500"
					/>
				)}
			</button>
			<button
				type="button"
				onClick={onClose}
				aria-label={`Close ${display.label}`}
				title="Close tab (Ctrl+W)"
				className="ml-1 grid size-4 shrink-0 place-items-center rounded text-muted-foreground/70 opacity-0 transition-opacity cursor-pointer hover:bg-foreground/10 hover:text-foreground group-hover/tab:opacity-100 data-[active=true]:opacity-100"
				data-active={active || undefined}
			>
				<X className="size-3" strokeWidth={1.8} />
			</button>
		</div>
	);
}
