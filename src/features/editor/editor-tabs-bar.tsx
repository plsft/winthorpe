import { MessageSquareMore, Save, X } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
	Tooltip,
	TooltipContent,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import { describeEditorPath } from "@/lib/editor-session";
import { cn } from "@/lib/utils";
import type { OpenTab } from "./tabbed-editor-host";

/**
 * The tab strip rendered at the top of the workspace viewport whenever any
 * file tabs are open OR the user is on the chat view but has open tabs to
 * potentially switch back to. Pinned leftmost: a non-closable, non-draggable
 * "Chat" tab that returns the viewport to the conversation view.
 *
 * The bar is intentionally view-mode-agnostic — it only knows about:
 *   - Whether the chat tab is active (`chatActive`)
 *   - The list of file tabs + which one is active
 *
 * Callers map "user clicked Chat" → setWorkspaceViewMode("conversation"),
 * and "user clicked file tab X" → setWorkspaceViewMode("editor-tabs") +
 * setActiveTabId(X). The bar emits intents; mode is owned by App.
 */

export const CHAT_TAB_ID = "__chat__" as const;

interface EditorTabsBarProps {
	tabs: OpenTab[];
	activeFileTabId: string | null;
	chatActive: boolean;
	workspaceRootPath?: string | null;
	savingTabId?: string | null;
	onSelectChat: () => void;
	onSelectFileTab: (tabId: string) => void;
	onCloseFileTab: (tabId: string) => void;
	onReorderFileTab: (fromTabId: string, toIndex: number) => void;
	/** Triggered by the inline Save button. Disabled when no file tab is
	    active or when the active tab isn't dirty. */
	onSaveActiveFile?: () => void;
	activeFileIsDirty?: boolean;
}

const TAB_DND_TYPE = "application/x-winthorpe-tab";

export function EditorTabsBar({
	tabs,
	activeFileTabId,
	chatActive,
	workspaceRootPath,
	savingTabId,
	onSelectChat,
	onSelectFileTab,
	onCloseFileTab,
	onReorderFileTab,
	onSaveActiveFile,
	activeFileIsDirty,
}: EditorTabsBarProps) {
	const showSave = !chatActive && !!activeFileTabId;
	return (
		<div
			className="flex h-9 shrink-0 items-end overflow-x-auto border-b border-border bg-background/60"
			role="tablist"
			aria-label="Workspace tabs"
		>
			{/* Pinned Chat tab — always present, non-closable, non-draggable. */}
			<ChatTab active={chatActive} onClick={onSelectChat} />

			{tabs.map((tab, index) => (
				<FileTabButton
					key={tab.id}
					tab={tab}
					index={index}
					active={!chatActive && tab.id === activeFileTabId}
					saving={savingTabId === tab.id}
					workspaceRootPath={workspaceRootPath ?? null}
					onFocus={() => onSelectFileTab(tab.id)}
					onClose={() => onCloseFileTab(tab.id)}
					onReorder={onReorderFileTab}
				/>
			))}

			<div className="flex flex-1 items-center justify-end gap-1 border-b border-border px-2">
				{showSave && (
					<Tooltip>
						<TooltipTrigger asChild>
							<Button
								type="button"
								variant="ghost"
								size="icon-xs"
								disabled={!activeFileIsDirty || savingTabId !== null}
								onClick={onSaveActiveFile}
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
				)}
			</div>
		</div>
	);
}

function ChatTab({
	active,
	onClick,
}: {
	active: boolean;
	onClick: () => void;
}) {
	return (
		<button
			type="button"
			role="tab"
			aria-selected={active}
			data-active={active || undefined}
			onClick={onClick}
			title="Chat (conversation view)"
			className={cn(
				"flex h-full shrink-0 items-center gap-1.5 border-r border-border px-3 text-[12px] transition-colors cursor-pointer focus-visible:outline-none",
				active
					? "bg-background text-foreground"
					: "bg-background/30 text-muted-foreground hover:bg-foreground/5",
			)}
		>
			<MessageSquareMore className="size-3.5" strokeWidth={1.6} />
			<span>Chat</span>
		</button>
	);
}

interface FileTabButtonProps {
	tab: OpenTab;
	index: number;
	active: boolean;
	saving: boolean;
	workspaceRootPath: string | null;
	onFocus: () => void;
	onClose: () => void;
	onReorder: (fromTabId: string, toIndex: number) => void;
}

function FileTabButton({
	tab,
	index,
	active,
	saving,
	workspaceRootPath,
	onFocus,
	onClose,
	onReorder,
}: FileTabButtonProps) {
	const display = useMemo(() => {
		const rel = describeEditorPath(tab.session.path, workspaceRootPath);
		const slash = rel.lastIndexOf("/");
		return {
			label: slash === -1 ? rel : rel.slice(slash + 1),
			tooltip: rel,
		};
	}, [tab.session.path, workspaceRootPath]);

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
