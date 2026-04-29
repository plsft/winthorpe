import { useQuery } from "@tanstack/react-query";
import { ChevronRight, File as FileIcon, FolderOpen } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { listWorkspaceTree, type WorkspaceTreeEntry } from "@/lib/api";
import { cn } from "@/lib/utils";

/**
 * Workspace file explorer.
 *
 * Loads `list_workspace_tree` (a flat pre-order list of every non-ignored
 * entry under the workspace root), groups it into a hierarchical tree
 * client-side, and renders it as collapsible folders. Click a file →
 * `onOpenFile(absolutePath)` callback (the editor host owns tab state).
 *
 * Folder expand/collapse state lives in component state — persisted per
 * workspace via the `workspaceId` key so a user's expansion choices
 * survive the workspace switch.
 */

interface FileTreeProps {
	workspaceId: string;
	workspaceRootPath: string;
	activeFilePath?: string | null;
	onOpenFile: (entry: WorkspaceTreeEntry) => void;
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
	className,
}: FileTreeProps) {
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

	// Per-workspace expansion state. Keyed by relative path. Default: empty
	// (everything collapsed). Top-level dirs auto-expand on first load
	// because users almost always want to see the immediate children.
	const [expanded, setExpanded] = useState<Record<string, Set<string>>>({});

	const expandedForWorkspace = expanded[workspaceId] ?? new Set<string>();
	const toggleExpand = useCallback(
		(path: string) => {
			setExpanded((prev) => {
				const current = new Set(prev[workspaceId] ?? []);
				if (current.has(path)) {
					current.delete(path);
				} else {
					current.add(path);
				}
				return { ...prev, [workspaceId]: current };
			});
		},
		[workspaceId],
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
			<div
				className={cn(
					"flex h-full items-center justify-center px-4 text-center text-[12px] text-muted-foreground",
					className,
				)}
			>
				This workspace is empty.
			</div>
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
	onToggle: (path: string) => void;
	onOpenFile: (entry: WorkspaceTreeEntry) => void;
}

function TreeNodeRow({
	node,
	depth,
	expanded,
	activeFilePath,
	onToggle,
	onOpenFile,
}: TreeNodeRowProps) {
	const { entry, children } = node;
	const isDir = entry.kind === "directory";
	const isOpen = expanded.has(entry.path);
	const isActive = !isDir && activeFilePath === entry.absolutePath;

	const handleClick = useCallback(() => {
		if (isDir) {
			onToggle(entry.path);
		} else {
			onOpenFile(entry);
		}
	}, [isDir, entry, onToggle, onOpenFile]);

	const handleKeyDown = useCallback(
		(event: React.KeyboardEvent<HTMLButtonElement>) => {
			if (event.key === "Enter" || event.key === " ") {
				event.preventDefault();
				handleClick();
			}
		},
		[handleClick],
	);

	// Indentation: 12px per level. Cap at depth 12 so deeply nested trees
	// don't push content off the right edge of the sidebar.
	const indentPx = Math.min(depth, 12) * 12;

	return (
		<>
			<button
				type="button"
				onClick={handleClick}
				onKeyDown={handleKeyDown}
				role="treeitem"
				aria-expanded={isDir ? isOpen : undefined}
				aria-selected={isActive}
				title={entry.path}
				className={cn(
					"flex w-full items-center gap-1 px-2 py-[3px] text-left transition-colors cursor-pointer",
					"hover:bg-foreground/5",
					"focus-visible:outline-none focus-visible:bg-foreground/8",
					isActive && "bg-foreground/10 text-foreground",
				)}
				style={{ paddingLeft: `${8 + indentPx}px` }}
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
				<span className="truncate">{entry.name}</span>
			</button>

			{isDir && isOpen && children.length > 0 && (
				<>
					{children.map((child) => (
						<TreeNodeRow
							key={child.entry.path}
							node={child}
							depth={depth + 1}
							expanded={expanded}
							onToggle={onToggle}
							onOpenFile={onOpenFile}
							activeFilePath={activeFilePath}
						/>
					))}
				</>
			)}
		</>
	);
}

/**
 * Build a hierarchical tree from the flat pre-order list. Backend already
 * sorts (dirs first, then files, alphabetical), so we just attach each
 * entry to its parent. Pre-order means a directory always appears before
 * its children, which lets us walk linearly with a stack of "current dir"
 * frames — no second sort, no second pass.
 */
function buildTree(entries: WorkspaceTreeEntry[]): TreeNode[] {
	const roots: TreeNode[] = [];
	// Stack of (path, children-array). The root frame has path "" and
	// pushes top-level entries into `roots`.
	const stack: Array<{ path: string; children: TreeNode[] }> = [
		{ path: "", children: roots },
	];

	for (const entry of entries) {
		const parentPath = parentPathOf(entry.path);
		// Pop frames until the top matches the current entry's parent.
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
