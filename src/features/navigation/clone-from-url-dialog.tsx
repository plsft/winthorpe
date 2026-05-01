import { open } from "@tauri-apps/plugin-dialog";
import { Globe, LoaderCircle, Lock, RefreshCw, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
	type GithubRepositorySummary,
	listGithubAccessibleRepositories,
} from "@/lib/api";
import { cn } from "@/lib/utils";
import { describeUnknownError } from "@/lib/workspace-helpers";

type SubmitArgs = {
	gitUrl: string;
	cloneDirectory: string;
};

type CloneFromUrlDialogProps = {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	defaultCloneDirectory: string | null;
	onSubmit: (args: SubmitArgs) => Promise<void>;
};

type Mode = "github" | "url";

export function CloneFromUrlDialog({
	open: isOpen,
	onOpenChange,
	defaultCloneDirectory,
	onSubmit,
}: CloneFromUrlDialogProps) {
	const [mode, setMode] = useState<Mode>("github");
	const [gitUrl, setGitUrl] = useState("");
	const [cloneDirectory, setCloneDirectory] = useState("");
	const [isSubmitting, setIsSubmitting] = useState(false);
	const [errorMessage, setErrorMessage] = useState<string | null>(null);

	// GitHub tab state
	const [repos, setRepos] = useState<GithubRepositorySummary[] | null>(null);
	const [reposLoading, setReposLoading] = useState(false);
	const [reposError, setReposError] = useState<string | null>(null);
	const [filterText, setFilterText] = useState("");
	const [selectedRepoId, setSelectedRepoId] = useState<number | null>(null);

	// Track whether the user has explicitly edited the location so the default
	// only seeds the field once per open session.
	const cloneDirectoryTouchedRef = useRef(false);

	useEffect(() => {
		if (!isOpen) return;
		setIsSubmitting(false);
		setErrorMessage(null);
		if (!cloneDirectoryTouchedRef.current) {
			setCloneDirectory(defaultCloneDirectory ?? "");
		}
	}, [isOpen, defaultCloneDirectory]);

	const loadRepos = useCallback(async () => {
		setReposLoading(true);
		setReposError(null);
		try {
			const list = await listGithubAccessibleRepositories();
			// Keep the GitHub backend's order (updated desc) — most-recently-
			// touched repos are usually what the user is reaching for.
			setRepos(list);
		} catch (error) {
			setReposError(
				describeUnknownError(
					error,
					"Couldn't load your GitHub repositories. Are you signed in?",
				),
			);
		} finally {
			setReposLoading(false);
		}
	}, []);

	// Lazy-load repo list the first time the GitHub tab is shown each open.
	useEffect(() => {
		if (!isOpen) return;
		if (mode !== "github") return;
		if (repos !== null || reposLoading) return;
		void loadRepos();
	}, [isOpen, mode, repos, reposLoading, loadRepos]);

	const handleBrowse = useCallback(async () => {
		try {
			const selection = await open({
				directory: true,
				multiple: false,
				defaultPath: cloneDirectory || defaultCloneDirectory || undefined,
			});
			const selected = Array.isArray(selection) ? selection[0] : selection;
			if (selected) {
				cloneDirectoryTouchedRef.current = true;
				setCloneDirectory(selected);
			}
		} catch (error) {
			setErrorMessage(
				describeUnknownError(error, "Unable to open the folder picker."),
			);
		}
	}, [cloneDirectory, defaultCloneDirectory]);

	const trimmedDirectory = cloneDirectory.trim();
	const trimmedUrl = gitUrl.trim();

	const selectedRepo = useMemo(
		() => repos?.find((r) => r.id === selectedRepoId) ?? null,
		[repos, selectedRepoId],
	);

	const filteredRepos = useMemo(() => {
		if (!repos) return [];
		const needle = filterText.trim().toLowerCase();
		if (!needle) return repos;
		return repos.filter(
			(r) =>
				r.fullName.toLowerCase().includes(needle) ||
				r.name.toLowerCase().includes(needle) ||
				r.ownerLogin.toLowerCase().includes(needle),
		);
	}, [repos, filterText]);

	const canSubmit =
		trimmedDirectory.length > 0 &&
		!isSubmitting &&
		(mode === "url" ? trimmedUrl.length > 0 : selectedRepo !== null);

	const handleSubmit = useCallback(async () => {
		if (!canSubmit) return;
		const url =
			mode === "github" && selectedRepo ? selectedRepo.htmlUrl : trimmedUrl;
		setIsSubmitting(true);
		setErrorMessage(null);
		try {
			await onSubmit({ gitUrl: url, cloneDirectory: trimmedDirectory });
			// Reset on success so the next open is clean.
			setGitUrl("");
			setCloneDirectory("");
			setSelectedRepoId(null);
			setFilterText("");
			cloneDirectoryTouchedRef.current = false;
			onOpenChange(false);
		} catch (error) {
			setErrorMessage(
				describeUnknownError(error, "Unable to clone repository."),
			);
		} finally {
			setIsSubmitting(false);
		}
	}, [
		canSubmit,
		mode,
		onOpenChange,
		onSubmit,
		selectedRepo,
		trimmedDirectory,
		trimmedUrl,
	]);

	return (
		<Dialog
			open={isOpen}
			onOpenChange={(nextOpen) => {
				if (isSubmitting && !nextOpen) return;
				onOpenChange(nextOpen);
			}}
		>
			<DialogContent className="gap-3 p-4 sm:max-w-2xl">
				<DialogHeader>
					<DialogTitle className="text-[13px] font-medium tracking-[-0.01em]">
						Clone a repository
					</DialogTitle>
				</DialogHeader>

				{/* Tab strip */}
				<div className="flex items-center gap-1 border-b border-border/60 pb-2">
					<TabButton
						active={mode === "github"}
						onClick={() => setMode("github")}
					>
						<svg
							viewBox="0 0 16 16"
							className="size-3.5"
							fill="currentColor"
							aria-hidden="true"
						>
							<path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.012 8.012 0 0 0 16 8c0-4.42-3.58-8-8-8z" />
						</svg>
						From your GitHub
					</TabButton>
					<TabButton active={mode === "url"} onClick={() => setMode("url")}>
						From URL
					</TabButton>
				</div>

				<form
					onSubmit={(event) => {
						event.preventDefault();
						void handleSubmit();
					}}
					className="flex flex-col gap-3"
				>
					{mode === "github" ? (
						<GithubRepoPicker
							repos={filteredRepos}
							totalRepos={repos?.length ?? 0}
							loading={reposLoading}
							error={reposError}
							filterText={filterText}
							onFilterTextChange={setFilterText}
							selectedRepoId={selectedRepoId}
							onSelect={setSelectedRepoId}
							onRefresh={() => {
								setRepos(null);
								void loadRepos();
							}}
							disabled={isSubmitting}
						/>
					) : (
						<div className="flex flex-col gap-1">
							<Label
								htmlFor="clone-git-url"
								className="text-[12px] font-medium tracking-[-0.01em]"
							>
								Git URL
							</Label>
							<Input
								id="clone-git-url"
								type="text"
								value={gitUrl}
								onChange={(event) => setGitUrl(event.target.value)}
								placeholder="https://github.com/user/repo.git or git@github.com:user/repo.git"
								autoFocus
								autoComplete="off"
								autoCorrect="off"
								spellCheck={false}
								disabled={isSubmitting}
								className="h-7 text-[13px] md:text-[13px]"
							/>
						</div>
					)}

					<div className="flex flex-col gap-1">
						<Label
							htmlFor="clone-location"
							className="text-[12px] font-medium tracking-[-0.01em]"
						>
							Clone location
						</Label>
						<div className="flex items-center gap-1.5">
							<Input
								id="clone-location"
								type="text"
								value={cloneDirectory}
								onChange={(event) => {
									cloneDirectoryTouchedRef.current = true;
									setCloneDirectory(event.target.value);
								}}
								placeholder="e.g. C:\Users\you\Code"
								autoComplete="off"
								autoCorrect="off"
								spellCheck={false}
								disabled={isSubmitting}
								className="h-7 text-[13px] md:text-[13px]"
							/>
							<Button
								type="button"
								variant="outline"
								size="sm"
								onClick={() => void handleBrowse()}
								disabled={isSubmitting}
							>
								Browse…
							</Button>
						</div>
						<p className="text-[11px] text-muted-foreground">
							Folder is created if it doesn't exist. The repo is cloned into a
							subfolder named after itself.
						</p>
					</div>

					{errorMessage ? (
						<p
							role="alert"
							className="text-destructive text-[12px] leading-snug"
						>
							{errorMessage}
						</p>
					) : null}
					<div className="flex justify-end pt-0.5">
						<Button type="submit" size="sm" disabled={!canSubmit}>
							{isSubmitting ? (
								<>
									<LoaderCircle className="animate-spin" strokeWidth={2.1} />
									Cloning…
								</>
							) : mode === "github" && selectedRepo ? (
								`Clone ${selectedRepo.fullName}`
							) : (
								"Clone repository"
							)}
						</Button>
					</div>
				</form>
			</DialogContent>
		</Dialog>
	);
}

function TabButton({
	active,
	onClick,
	children,
}: {
	active: boolean;
	onClick: () => void;
	children: React.ReactNode;
}) {
	return (
		<button
			type="button"
			onClick={onClick}
			className={cn(
				"flex items-center gap-1.5 rounded-md px-2.5 py-1 text-[12px] font-medium transition-colors",
				active
					? "bg-muted text-foreground"
					: "text-muted-foreground hover:bg-muted/60 hover:text-foreground",
			)}
		>
			{children}
		</button>
	);
}

function GithubRepoPicker({
	repos,
	totalRepos,
	loading,
	error,
	filterText,
	onFilterTextChange,
	selectedRepoId,
	onSelect,
	onRefresh,
	disabled,
}: {
	repos: GithubRepositorySummary[];
	totalRepos: number;
	loading: boolean;
	error: string | null;
	filterText: string;
	onFilterTextChange: (value: string) => void;
	selectedRepoId: number | null;
	onSelect: (id: number) => void;
	onRefresh: () => void;
	disabled: boolean;
}) {
	return (
		<div className="flex flex-col gap-2">
			<div className="relative">
				<Search className="absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
				<Input
					type="search"
					value={filterText}
					onChange={(event) => onFilterTextChange(event.target.value)}
					placeholder="Filter your repos…"
					autoComplete="off"
					autoCorrect="off"
					spellCheck={false}
					disabled={disabled}
					className="h-7 pl-7 text-[13px] md:text-[13px]"
				/>
			</div>
			<div className="rounded-md border border-border/60 bg-card">
				<div className="flex items-center justify-between border-b border-border/40 px-3 py-1.5 text-[11px] text-muted-foreground">
					<span>
						{loading
							? "Loading…"
							: totalRepos === 0
								? "No repositories"
								: `${repos.length} of ${totalRepos} repositories`}
					</span>
					<button
						type="button"
						onClick={onRefresh}
						disabled={loading || disabled}
						className="flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-muted hover:text-foreground disabled:opacity-50"
						title="Refresh list"
					>
						<RefreshCw className={cn("size-3", loading && "animate-spin")} />
						Refresh
					</button>
				</div>

				{error ? (
					<div className="px-3 py-4 text-[12px] text-destructive">{error}</div>
				) : loading && repos.length === 0 ? (
					<div className="flex items-center justify-center px-3 py-8 text-[12px] text-muted-foreground">
						<LoaderCircle className="mr-2 size-3.5 animate-spin" />
						Fetching your repos…
					</div>
				) : repos.length === 0 ? (
					<div className="px-3 py-4 text-[12px] text-muted-foreground">
						{filterText
							? "No matches. Try a different filter."
							: "No repositories returned. Connect GitHub in Settings."}
					</div>
				) : (
					<ul className="max-h-72 overflow-y-auto">
						{repos.map((repo) => {
							const selected = repo.id === selectedRepoId;
							return (
								<li key={repo.id}>
									<button
										type="button"
										onClick={() => onSelect(repo.id)}
										disabled={disabled}
										className={cn(
											"flex w-full items-start gap-2 border-b border-border/30 px-3 py-2 text-left transition-colors last:border-b-0",
											selected ? "bg-primary/10" : "hover:bg-muted/60",
										)}
									>
										{repo.private ? (
											<Lock className="mt-0.5 size-3.5 shrink-0 text-amber-500" />
										) : (
											<Globe className="mt-0.5 size-3.5 shrink-0 text-emerald-500" />
										)}
										<div className="min-w-0 flex-1">
											<div className="truncate text-[13px] font-medium text-foreground">
												{repo.fullName}
											</div>
											<div className="truncate text-[11px] text-muted-foreground">
												{repo.defaultBranch ?? "main"}
												{repo.updatedAt
													? ` · updated ${formatRelativeTime(repo.updatedAt)}`
													: ""}
											</div>
										</div>
									</button>
								</li>
							);
						})}
					</ul>
				)}
			</div>
		</div>
	);
}

function formatRelativeTime(iso: string): string {
	const then = new Date(iso).getTime();
	if (Number.isNaN(then)) return "";
	const diffMs = Date.now() - then;
	const minute = 60_000;
	const hour = 60 * minute;
	const day = 24 * hour;
	if (diffMs < minute) return "just now";
	if (diffMs < hour) return `${Math.round(diffMs / minute)}m ago`;
	if (diffMs < day) return `${Math.round(diffMs / hour)}h ago`;
	if (diffMs < 30 * day) return `${Math.round(diffMs / day)}d ago`;
	return new Date(iso).toLocaleDateString();
}
