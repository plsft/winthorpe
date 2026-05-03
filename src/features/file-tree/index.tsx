import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
	ChevronRight,
	File as FileIcon,
	FilePlus,
	FolderOpen,
	FolderPlus,
	Pencil,
	Trash2,
} from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import {
	ContextMenu,
	ContextMenuContent,
	ContextMenuItem,
	ContextMenuSeparator,
	ContextMenuTrigger,
} from "@/components/ui/context-menu";
import {
	createWorkspaceDirectory,
	createWorkspaceFile,
	deleteWorkspacePath,
	listWorkspaceTree,
	renameWorkspacePath,
	type WorkspaceTreeEntry,
} from "@/lib/api";
import { cn } from "@/lib/utils";

/**
 * Workspace file explorer with right-click context menu.
 *
 * Loads `list_workspace_tree` (a flat pre-order list of every non-ignored
 * entry under the workspace root), groups it into a hierarchical tree
 * client-side, and renders it as collapsible folders.
 *
 * Interactions:
 *   - Click a file → `onOpenFile(entry)` (the editor host owns tab state)
 *   - Click a folder ▶ → toggle expand
 *   - Right-click anywhere → context menu:
 *       - New file (sibling, or child if right-clicked on a folder)
 *       - New folder (same)
 *       - Rename — inline input
 *       - Delete — confirm dialog, propagates to onPathDeleted callback
 *         so the editor host can close any open tabs for the deleted path
 *
 * Folder expand state lives in component state, persisted per workspace
 * via the `workspaceId` key.
 */

interface FileTreeProps {
	workspaceId: string;
	workspaceRootPath: string;
	activeFilePath?: string | null;
	onOpenFile: (entry: WorkspaceTreeEntry) => void;
	/** Called after a successful delete or rename so callers can close
	    matching open editor tabs. Passes ABSOLUTE paths. */
	onPathRemoved?: (absolutePath: string) => void;
	className?: string;
}

interface TreeNode {
	entry: WorkspaceTreeEntry;
	children: TreeNode[];
}

export function FileTree({
	workspaceId,
	workspaceRootPath,
	activeFilePath,
	onOpenFile,
	onPathRemoved,
	className,
}: FileTreeProps) {
	const queryClient = useQueryClient();
	const query = useQuery({
		queryKey: ["workspaceTree", workspaceRootPath],
		queryFn: () => listWorkspaceTree(workspaceRootPath),
		enabled: workspaceRootPath.length > 0,
		staleTime: 30_000,
	});

	const tree = useMemo<TreeNode[]>(
		() => (query.data ? buildTree(query.data) : []),
		[query.data],
	);

	const [expanded, setExpanded] = useState<Record<string, Set<string>>>({});
	const expandedForWorkspace = expanded[workspaceId] ?? new Set<string>();
	const toggleExpand = useCallback(
		(path: string) => {
			setExpanded((prev) => {
				const current = new Set(prev[workspaceId] ?? []);
				if (current.has(path)) current.delete(path);
				else current.add(path);
				return { ...prev, [workspaceId]: current };
			});
		},
		[workspaceId],
	);
	const ensureExpanded = useCallback(
		(path: string) => {
			setExpanded((prev) => {
				const current = new Set(prev[workspaceId] ?? []);
				if (current.has(path)) return prev;
				current.add(path);
				return { ...prev, [workspaceId]: current };
			});
		},
		[workspaceId],
	);

	// Inline-rename state. Only one row can be in rename mode at a time.
	const [renamingPath, setRenamingPath] = useState<string | null>(null);

	const refreshTree = useCallback(() => {
		void queryClient.invalidateQueries({
			queryKey: ["workspaceTree", workspaceRootPath],
		});
	}, [queryClient, workspaceRootPath]);

	// New-file / new-folder modal state — keep it dead simple with native prompt.
	const promptThenCreate = useCallback(
		async (parentAbsolute: string, kind: "file" | "directory") => {
			const promptLabel = kind === "file" ? "New file name" : "New folder name";
			const name = window.prompt(promptLabel)?.trim();
			if (!name) return;
			if (name.includes("/") || name.includes("\\")) {
				toast.error("Name can't contain slashes");
				return;
			}
			// Join parent + name with the workspace's path separator. We store
			// absolute paths with whichever style the OS gave us (Windows uses
			// backslash); Tauri's resolve_allowed_path normalises both.
			const sep = parentAbsolute.includes("\\") ? "\\" : "/";
			const target = `${parentAbsolute}${sep}${name}`;
			try {
				if (kind === "file") {
					const created = await createWorkspaceFile(target);
					refreshTree();
					// Auto-expand the parent so the new file is visible.
					const parentRel = relativePathOf(parentAbsolute, workspaceRootPath);
					if (parentRel) ensureExpanded(parentRel);
					// Open the new file in the editor immediately.
					onOpenFile({
						path:
							relativePathOf(created.absolutePath, workspaceRootPath) ?? name,
						name,
						absolutePath: created.absolutePath,
						kind: "file",
					});
				} else {
					await createWorkspaceDirectory(target);
					refreshTree();
					const parentRel = relativePathOf(parentAbsolute, workspaceRootPath);
					if (parentRel) ensureExpanded(parentRel);
				}
			} catch (error) {
				toast.error(
					error instanceof Error ? error.message : `Failed to create ${kind}`,
				);
			}
		},
		[ensureExpanded, onOpenFile, refreshTree, workspaceRootPath],
	);

	const handleConfirmRename = useCallback(
		async (entry: WorkspaceTreeEntry, nextName: string) => {
			setRenamingPath(null);
			const trimmed = nextName.trim();
			if (!trimmed || trimmed === entry.name) return;
			if (trimmed.includes("/") || trimmed.includes("\\")) {
				toast.error("Name can't contain slashes");
				return;
			}
			const sep = entry.absolutePath.includes("\\") ? "\\" : "/";
			const parent = entry.absolutePath.slice(
				0,
				entry.absolutePath.lastIndexOf(sep),
			);
			const target = `${parent}${sep}${trimmed}`;
			try {
				await renameWorkspacePath(entry.absolutePath, target);
				refreshTree();
				onPathRemoved?.(entry.absolutePath);
			} catch (error) {
				toast.error(
					error instanceof Error ? error.message : "Failed to rename",
				);
			}
		},
		[onPathRemoved, refreshTree],
	);

	const handleDelete = useCallback(
		async (entry: WorkspaceTreeEntry) => {
			const proceed = window.confirm(
				`Delete ${entry.kind === "directory" ? "folder" : "file"} "${entry.name}"?\n\n${entry.absolutePath}`,
			);
			if (!proceed) return;
			try {
				await deleteWorkspacePath(entry.absolutePath);
				refreshTree();
				onPathRemoved?.(entry.absolutePath);
			} catch (error) {
				toast.error(
					error instanceof Error ? error.message : "Failed to delete",
				);
			}
		},
		[onPathRemoved, refreshTree],
	);

	if (query.isLoading) {
		return (
			<div
				className={cn(
					"flex h-full items-center justify-center text-[12px] text-muted-foreground",
					className,
				)}
			>
				Loading…
			</div>
		);
	}

	if (query.isError) {
		return (
			<div
				className={cn(
					"flex h-full flex-col items-center justify-center gap-2 px-4 text-center text-[12px] text-muted-foreground",
					className,
				)}
			>
				<span>Couldn't load files.</span>
				<button
					type="button"
					onClick={() => void query.refetch()}
					className="text-[11px] underline cursor-pointer hover:text-foreground"
				>
					Retry
				</button>
			</div>
		);
	}

	if (tree.length === 0) {
		return (
			<EmptyTreeWithRootMenu
				workspaceRootPath={workspaceRootPath}
				className={className}
				onNewFile={() => promptThenCreate(workspaceRootPath, "file")}
				onNewFolder={() => promptThenCreate(workspaceRootPath, "directory")}
			/>
		);
	}

	return (
		<div
			className={cn(
				"flex h-full min-h-0 flex-col overflow-y-auto py-1.5 text-[12px] text-foreground/85",
				className,
			)}
			data-slot="file-tree"
			role="tree"
			aria-label="Workspace files"
		>
			{tree.map((node) => (
				<TreeNodeRow
					key={node.entry.path}
					node={node}
					depth={0}
					expanded={expandedForWorkspace}
					onToggle={toggleExpand}
					onOpenFile={onOpenFile}
					activeFilePath={activeFilePath ?? null}
					renamingPath={renamingPath}
					onStartRename={setRenamingPath}
					onConfirmRename={handleConfirmRename}
					onCancelRename={() => setRenamingPath(null)}
					onDelete={handleDelete}
					onNewFileInParent={(parent) => promptThenCreate(parent, "file")}
					onNewFolderInParent={(parent) =>
						promptThenCreate(parent, "directory")
					}
					workspaceRootPath={workspaceRootPath}
				/>
			))}
		</div>
	);
}

interface TreeNodeRowProps {
	node: TreeNode;
	depth: number;
	expanded: Set<string>;
	activeFilePath: string | null;
	renamingPath: string | null;
	workspaceRootPath: string;
	onToggle: (path: string) => void;
	onOpenFile: (entry: WorkspaceTreeEntry) => void;
	onStartRename: (path: string) => void;
	onConfirmRename: (entry: WorkspaceTreeEntry, nextName: string) => void;
	onCancelRename: () => void;
	onDelete: (entry: WorkspaceTreeEntry) => void;
	onNewFileInParent: (parentAbsolutePath: string) => void;
	onNewFolderInParent: (parentAbsolutePath: string) => void;
}

function TreeNodeRow(props: TreeNodeRowProps) {
	const {
		node,
		depth,
		expanded,
		activeFilePath,
		renamingPath,
		workspaceRootPath,
		onToggle,
		onOpenFile,
		onStartRename,
		onConfirmRename,
		onCancelRename,
		onDelete,
		onNewFileInParent,
		onNewFolderInParent,
	} = props;
	const { entry, children } = node;
	const isDir = entry.kind === "directory";
	const isOpen = expanded.has(entry.path);
	const isActive = !isDir && activeFilePath === entry.absolutePath;
	const isRenaming = renamingPath === entry.absolutePath;

	const handleClick = useCallback(() => {
		if (isDir) onToggle(entry.path);
		else onOpenFile(entry);
	}, [isDir, entry, onToggle, onOpenFile]);

	const indentPx = Math.min(depth, 12) * 12;

	// Where to create new siblings: when the user right-clicks on a folder,
	// create INSIDE it. When right-clicking on a file, create as its sibling
	// (i.e. inside its parent directory).
	const newItemParent = isDir
		? entry.absolutePath
		: parentAbsoluteOf(entry.absolutePath);

	return (
		<>
			<ContextMenu>
				<ContextMenuTrigger asChild>
					<div
						className={cn(
							"flex w-full items-center gap-1 px-2 py-[3px] text-left transition-colors cursor-pointer",
							"hover:bg-foreground/5",
							"focus-visible:outline-none focus-visible:bg-foreground/8",
							isActive && "bg-foreground/10 text-foreground",
						)}
						style={{ paddingLeft: `${8 + indentPx}px` }}
						onClick={handleClick}
						onKeyDown={(event) => {
							if (event.key === "Enter" || event.key === " ") {
								event.preventDefault();
								handleClick();
							}
							if (event.key === "F2" && !isRenaming) {
								event.preventDefault();
								onStartRename(entry.absolutePath);
							}
							if (event.key === "Delete") {
								event.preventDefault();
								onDelete(entry);
							}
						}}
						role="treeitem"
						aria-expanded={isDir ? isOpen : undefined}
						aria-selected={isActive}
						title={entry.path}
						tabIndex={0}
					>
						{isDir ? (
							<>
								<ChevronRight
									className={cn(
										"size-3 shrink-0 text-muted-foreground transition-transform",
										isOpen && "rotate-90",
									)}
									strokeWidth={2}
								/>
								<FolderOpen
									className="size-3.5 shrink-0 text-muted-foreground/85"
									strokeWidth={1.6}
								/>
							</>
						) : (
							<>
								<span className="size-3 shrink-0" aria-hidden="true" />
								<FileIcon
									className="size-3.5 shrink-0 text-muted-foreground/70"
									strokeWidth={1.5}
								/>
							</>
						)}
						{isRenaming ? (
							<RenameInput
								initialValue={entry.name}
								onCommit={(value) => onConfirmRename(entry, value)}
								onCancel={onCancelRename}
							/>
						) : (
							<span className="truncate">{entry.name}</span>
						)}
					</div>
				</ContextMenuTrigger>
				<ContextMenuContent className="min-w-[180px]">
					<ContextMenuItem
						onSelect={() => onNewFileInParent(newItemParent)}
						className="gap-2 text-[12px]"
					>
						<FilePlus className="size-3.5" strokeWidth={1.6} />
						New file
					</ContextMenuItem>
					<ContextMenuItem
						onSelect={() => onNewFolderInParent(newItemParent)}
						className="gap-2 text-[12px]"
					>
						<FolderPlus className="size-3.5" strokeWidth={1.6} />
						New folder
					</ContextMenuItem>
					<ContextMenuSeparator />
					<ContextMenuItem
						onSelect={() => onStartRename(entry.absolutePath)}
						className="gap-2 text-[12px]"
					>
						<Pencil className="size-3.5" strokeWidth={1.6} />
						Rename
						<span className="ml-auto text-[10px] text-muted-foreground">
							F2
						</span>
					</ContextMenuItem>
					<ContextMenuItem
						onSelect={() => onDelete(entry)}
						className="gap-2 text-[12px] text-red-500 focus:text-red-500"
					>
						<Trash2 className="size-3.5" strokeWidth={1.6} />
						Delete
						<span className="ml-auto text-[10px] text-muted-foreground">
							Del
						</span>
					</ContextMenuItem>
				</ContextMenuContent>
			</ContextMenu>

			{isDir &&
				isOpen &&
				children.length > 0 &&
				children.map((child) => (
					<TreeNodeRow
						key={child.entry.path}
						node={child}
						depth={depth + 1}
						expanded={expanded}
						onToggle={onToggle}
						onOpenFile={onOpenFile}
						activeFilePath={activeFilePath}
						renamingPath={renamingPath}
						workspaceRootPath={workspaceRootPath}
						onStartRename={onStartRename}
						onConfirmRename={onConfirmRename}
						onCancelRename={onCancelRename}
						onDelete={onDelete}
						onNewFileInParent={onNewFileInParent}
						onNewFolderInParent={onNewFolderInParent}
					/>
				))}
		</>
	);
}

function RenameInput({
	initialValue,
	onCommit,
	onCancel,
}: {
	initialValue: string;
	onCommit: (value: string) => void;
	onCancel: () => void;
}) {
	const inputRef = useRef<HTMLInputElement>(null);
	const [value, setValue] = useState(initialValue);

	// Auto-focus + select-all once mounted so the user can immediately type
	// the new name. Select-all-but-extension is the common UX (e.g. Finder)
	// — a future enhancement.
	useMemo(() => {
		queueMicrotask(() => {
			inputRef.current?.focus();
			inputRef.current?.select();
		});
	}, []);

	return (
		<input
			ref={inputRef}
			value={value}
			onChange={(e) => setValue(e.target.value)}
			onKeyDown={(e) => {
				if (e.key === "Enter") {
					e.preventDefault();
					onCommit(value);
				}
				if (e.key === "Escape") {
					e.preventDefault();
					onCancel();
				}
			}}
			onBlur={() => onCommit(value)}
			onClick={(e) => e.stopPropagation()}
			className="min-w-0 flex-1 rounded-sm border border-input bg-background px-1 py-0 text-[12px] outline-none focus:ring-1 focus:ring-ring/40"
		/>
	);
}

function EmptyTreeWithRootMenu({
	className,
	onNewFile,
	onNewFolder,
}: {
	workspaceRootPath: string;
	className?: string;
	onNewFile: () => void;
	onNewFolder: () => void;
}) {
	return (
		<ContextMenu>
			<ContextMenuTrigger asChild>
				<div
					className={cn(
						"flex h-full flex-col items-center justify-center gap-2 px-4 text-center text-[12px] text-muted-foreground",
						className,
					)}
				>
					<span>This workspace is empty.</span>
					<span className="text-[11px] opacity-70">
						Right-click here to create a file or folder.
					</span>
				</div>
			</ContextMenuTrigger>
			<ContextMenuContent>
				<ContextMenuItem onSelect={onNewFile} className="gap-2 text-[12px]">
					<FilePlus className="size-3.5" strokeWidth={1.6} />
					New file
				</ContextMenuItem>
				<ContextMenuItem onSelect={onNewFolder} className="gap-2 text-[12px]">
					<FolderPlus className="size-3.5" strokeWidth={1.6} />
					New folder
				</ContextMenuItem>
			</ContextMenuContent>
		</ContextMenu>
	);
}

function buildTree(entries: WorkspaceTreeEntry[]): TreeNode[] {
	const roots: TreeNode[] = [];
	const stack: Array<{ path: string; children: TreeNode[] }> = [
		{ path: "", children: roots },
	];

	for (const entry of entries) {
		const parentPath = parentPathOf(entry.path);
		while (stack.length > 0 && stack[stack.length - 1]!.path !== parentPath) {
			stack.pop();
		}
		const top = stack[stack.length - 1] ?? { path: "", children: roots };
		const node: TreeNode = { entry, children: [] };
		top.children.push(node);
		if (entry.kind === "directory") {
			stack.push({ path: entry.path, children: node.children });
		}
	}

	return roots;
}

function parentPathOf(path: string): string {
	const slash = path.lastIndexOf("/");
	return slash === -1 ? "" : path.slice(0, slash);
}

function parentAbsoluteOf(absolutePath: string): string {
	const slash = Math.max(
		absolutePath.lastIndexOf("/"),
		absolutePath.lastIndexOf("\\"),
	);
	return slash === -1 ? absolutePath : absolutePath.slice(0, slash);
}

function relativePathOf(
	absolutePath: string,
	workspaceRootPath: string,
): string | null {
	const norm = (s: string) => s.replace(/\\/g, "/").replace(/\/+$/, "");
	const a = norm(absolutePath);
	const r = norm(workspaceRootPath);
	if (!a.startsWith(`${r}/`)) return null;
	return a.slice(r.length + 1);
}
