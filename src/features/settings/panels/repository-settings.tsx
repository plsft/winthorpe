import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
	Check,
	ChevronDown,
	GitBranch,
	HelpCircle,
	Trash2,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { BranchPickerPopover } from "@/components/branch-picker";
import { Button } from "@/components/ui/button";
import {
	Command,
	CommandEmpty,
	CommandItem,
	CommandList,
} from "@/components/ui/command";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Input } from "@/components/ui/input";
import {
	Popover,
	PopoverContent,
	PopoverTrigger,
} from "@/components/ui/popover";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import {
	Tooltip,
	TooltipContent,
	TooltipProvider,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import {
	deleteRepository,
	getForgeCliStatus,
	listRemoteBranches,
	listRepoRemotes,
	loadRepoScripts,
	prefetchRemoteRefs,
	type RepositoryCreateOption,
	updateRepoAutoRunSetup,
	updateRepoScripts,
	updateRepositoryBranchPrefix,
	updateRepositoryDefaultBranch,
	updateRepositoryRemote,
} from "@/lib/api";
import { useSettings } from "@/lib/settings";
import { cn, errorMessage } from "@/lib/utils";
import { SettingsGroup } from "../components/settings-row";
import { parseRemoteHost } from "./cli-install-gitlab-hosts";
import { RepositoryPreferencesSection } from "./repository-preferences-section";

export function RepositorySettingsPanel({
	repo,
	githubLogin,
	workspaceId,
	onRepoSettingsChanged,
	onRepoDeleted,
}: {
	repo: RepositoryCreateOption;
	githubLogin: string | null;
	workspaceId: string | null;
	onRepoSettingsChanged: () => void;
	onRepoDeleted: () => void;
}) {
	const [branches, setBranches] = useState<string[]>([]);
	const [loading, setLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const currentBranch = repo.defaultBranch ?? "main";

	const fetchBranches = useCallback(() => {
		setLoading(true);
		void listRemoteBranches({ repoId: repo.id })
			.then(setBranches)
			.finally(() => setLoading(false));
	}, [repo.id]);

	const handleOpen = useCallback(() => {
		fetchBranches();
		void prefetchRemoteRefs({ repoId: repo.id })
			.then(({ fetched }) => {
				if (fetched) fetchBranches();
			})
			.catch(() => {});
	}, [repo.id, fetchBranches]);

	const handleSelect = useCallback(
		(branch: string) => {
			if (branch === currentBranch) return;
			setError(null);
			void updateRepositoryDefaultBranch(repo.id, branch).then(
				onRepoSettingsChanged,
				(err: unknown) => {
					setError(errorMessage(err));
					onRepoSettingsChanged();
				},
			);
		},
		[repo.id, currentBranch, onRepoSettingsChanged],
	);

	const [remotes, setRemotes] = useState<string[]>([]);
	const [remoteOpen, setRemoteOpen] = useState(false);
	const [remoteError, setRemoteError] = useState<string | null>(null);
	const [remoteNotice, setRemoteNotice] = useState<string | null>(null);

	const currentRemote = repo.remote ?? "origin";

	const fetchRemotes = useCallback(() => {
		void listRepoRemotes(repo.id).then(setRemotes);
	}, [repo.id]);

	const handleRemoteSelect = useCallback(
		(remote: string) => {
			if (remote === currentRemote) return;
			setRemoteOpen(false);
			setRemoteError(null);
			setRemoteNotice(null);
			void updateRepositoryRemote(repo.id, remote).then(
				(response) => {
					if (response.orphanedWorkspaceCount > 0) {
						const n = response.orphanedWorkspaceCount;
						setRemoteNotice(
							`${n} workspace${n === 1 ? "" : "s"} target a branch not on this remote. Update them via the header branch picker.`,
						);
					}
					onRepoSettingsChanged();
				},
				(err: unknown) => {
					setRemoteError(errorMessage(err));
					onRepoSettingsChanged();
				},
			);
		},
		[repo.id, currentRemote, onRepoSettingsChanged],
	);

	return (
		<SettingsGroup>
			<div className="py-5">
				<div className="text-[13px] font-medium leading-snug text-foreground">
					Remote origin
				</div>
				<div className="mt-1 text-[12px] leading-snug text-muted-foreground">
					Where should we push, pull, and create PRs?
				</div>
				<div className="mt-3">
					<Popover
						open={remoteOpen}
						onOpenChange={(next: boolean) => {
							setRemoteOpen(next);
							if (next) fetchRemotes();
						}}
					>
						<PopoverTrigger className="inline-flex cursor-pointer items-center gap-1 rounded-lg border border-app-border/40 bg-app-base/30 px-3 py-2 text-[13px] font-medium text-app-foreground transition-colors hover:border-app-border-strong">
							<span className="truncate">{currentRemote}</span>
							<ChevronDown
								className="size-3 shrink-0 text-app-muted"
								strokeWidth={2}
							/>
						</PopoverTrigger>
						<PopoverContent align="start" className="w-[220px] p-0">
							<Command className="rounded-lg! p-0.5">
								<CommandList className="max-h-52">
									<CommandEmpty>No remotes found</CommandEmpty>
									{remotes.map((remote) => (
										<CommandItem
											key={remote}
											value={remote}
											onSelect={() => handleRemoteSelect(remote)}
											className="flex items-center justify-between gap-2 px-1.5 py-1 text-[12px]"
										>
											<span
												className={cn(
													"truncate",
													remote === currentRemote && "font-semibold",
												)}
											>
												{remote}
											</span>
											{remote === currentRemote && (
												<Check className="size-3.5 shrink-0" strokeWidth={2} />
											)}
										</CommandItem>
									))}
								</CommandList>
							</Command>
						</PopoverContent>
					</Popover>
					{remoteError && (
						<p className="mt-2 text-[12px] text-red-400/90">{remoteError}</p>
					)}
					{remoteNotice && (
						<p className="mt-2 text-[12px] text-amber-400/90">{remoteNotice}</p>
					)}
				</div>
			</div>

			<div className="py-5">
				<div className="text-[13px] font-medium leading-snug text-foreground">
					Branch new workspaces from
				</div>
				<div className="mt-1 text-[12px] leading-snug text-muted-foreground">
					Each workspace is an isolated copy of your codebase.
				</div>
				<div className="mt-3">
					<BranchPickerPopover
						currentBranch={currentBranch}
						branches={branches}
						loading={loading}
						onOpen={handleOpen}
						onSelect={handleSelect}
					>
						<button
							type="button"
							className="inline-flex cursor-pointer items-center gap-1 rounded-lg border border-app-border/40 bg-app-base/30 px-3 py-2 text-[13px] font-medium text-app-foreground transition-colors hover:border-app-border-strong"
						>
							<GitBranch
								className="size-3.5 text-app-foreground-soft"
								strokeWidth={1.8}
							/>
							<span className="truncate">
								{repo.remote ?? "origin"}/{currentBranch}
							</span>
							<ChevronDown
								className="size-3 shrink-0 text-app-muted"
								strokeWidth={2}
							/>
						</button>
					</BranchPickerPopover>
					{error && <p className="mt-2 text-[12px] text-red-400/90">{error}</p>}
				</div>
			</div>

			<BranchPrefixSection
				repo={repo}
				githubLogin={githubLogin}
				onChanged={onRepoSettingsChanged}
			/>

			<ScriptsSection repoId={repo.id} workspaceId={workspaceId} />
			<RepositoryPreferencesSection repoId={repo.id} />

			<DeleteRepoSection repo={repo} onDeleted={onRepoDeleted} />
		</SettingsGroup>
	);
}

function BranchPrefixSection({
	repo,
	githubLogin,
	onChanged,
}: {
	repo: RepositoryCreateOption;
	githubLogin: string | null;
	onChanged: () => void;
}) {
	const { settings } = useSettings();
	const [customPrefix, setCustomPrefix] = useState(
		repo.branchPrefixCustom ?? "",
	);
	const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
	const gitlabHost = parseRemoteHost(repo.remoteUrl);
	const isGitLabRepo =
		repo.forgeProvider === "gitlab" ||
		(repo.forgeProvider !== "github" && !!gitlabHost?.includes("gitlab"));
	const gitlabStatusQuery = useQuery({
		queryKey: ["forgeCliStatus", "gitlab", gitlabHost ?? "gitlab.com"],
		queryFn: () => getForgeCliStatus("gitlab", gitlabHost),
		// The global "github" option means "use the current provider login"
		// once it is resolved through this repository's forge.
		enabled: isGitLabRepo && settings.branchPrefixType === "github",
		staleTime: 2000,
	});
	const gitlabLogin =
		gitlabStatusQuery.data?.status === "ready"
			? gitlabStatusQuery.data.login
			: null;
	const inheritedPrefix = resolveInheritedBranchPrefix({
		type: settings.branchPrefixType,
		custom: settings.branchPrefixCustom,
		githubLogin,
		gitlabLogin,
		isGitLabRepo,
	});
	const inheritedPlaceholder = inheritedPrefix || "No prefix";

	useEffect(() => {
		setCustomPrefix(repo.branchPrefixCustom ?? "");
	}, [repo.id]);

	useEffect(() => {
		return () => {
			if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
		};
	}, [repo.id]);

	const save = useCallback(
		(nextCustom: string) => {
			if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
			saveTimerRef.current = setTimeout(() => {
				void updateRepositoryBranchPrefix(
					repo.id,
					nextCustom.trim() || null,
				).then(onChanged);
			}, 400);
		},
		[repo.id, onChanged],
	);

	return (
		<div className="py-5">
			<div className="text-[13px] font-medium leading-snug text-foreground">
				Branch prefix
			</div>
			<div className="mt-1 text-[12px] leading-snug text-muted-foreground">
				Leave empty to inherit the global prefix.
			</div>
			<div className="mt-4">
				<Input
					type="text"
					value={customPrefix}
					onChange={(event) => {
						const value = event.target.value;
						setCustomPrefix(value);
						save(value);
					}}
					placeholder={inheritedPlaceholder}
					className="w-full bg-app-base/30 text-[13px] text-app-foreground placeholder:text-muted-foreground/50"
				/>
				{customPrefix ? (
					<div className="mt-1.5 text-[12px] text-muted-foreground">
						Preview: {customPrefix}tokyo
					</div>
				) : null}
			</div>
		</div>
	);
}

function resolveInheritedBranchPrefix({
	type,
	custom,
	githubLogin,
	gitlabLogin,
	isGitLabRepo,
}: {
	type: "github" | "custom" | "none";
	custom: string;
	githubLogin: string | null;
	gitlabLogin: string | null;
	isGitLabRepo: boolean;
}): string {
	if (type === "custom") return custom.trim();
	if (type === "none") return "";
	const login = isGitLabRepo ? gitlabLogin : githubLogin;
	return login ? `${login}/` : "";
}

function ScriptField({
	label,
	description,
	placeholder,
	value,
	locked,
	lockedMessage,
	onChange,
	headerRight,
}: {
	label: string;
	description: string;
	placeholder: string;
	value: string;
	locked: boolean;
	lockedMessage: string;
	onChange: (e: React.ChangeEvent<HTMLTextAreaElement>) => void;
	headerRight?: React.ReactNode;
}) {
	const textarea = (
		<Textarea
			className="mt-2 min-h-[72px] resize-y bg-app-base/30 font-mono text-[12px]"
			placeholder={placeholder}
			value={value}
			onChange={onChange}
			readOnly={locked}
			disabled={locked}
		/>
	);

	return (
		<div>
			<div className="flex items-start justify-between gap-3">
				<div className="min-w-0">
					<div className="text-[12px] font-medium text-app-foreground">
						{label}
					</div>
					<div className="mt-0.5 text-[11px] text-muted-foreground">
						{description}
					</div>
				</div>
				{headerRight && <div className="shrink-0">{headerRight}</div>}
			</div>
			{locked ? (
				<TooltipProvider>
					<Tooltip>
						<TooltipTrigger asChild>{textarea}</TooltipTrigger>
						<TooltipContent side="top">{lockedMessage}</TooltipContent>
					</Tooltip>
				</TooltipProvider>
			) : (
				textarea
			)}
		</div>
	);
}

function ScriptsSection({
	repoId,
	workspaceId,
}: {
	repoId: string;
	workspaceId: string | null;
}) {
	const queryClient = useQueryClient();
	const scriptsQuery = useQuery({
		queryKey: ["repoScripts", repoId, workspaceId],
		queryFn: () => loadRepoScripts(repoId, workspaceId),
		staleTime: 0,
	});

	const data = scriptsQuery.data;
	const setupLocked = data?.setupFromProject ?? false;
	const runLocked = data?.runFromProject ?? false;
	const archiveLocked = data?.archiveFromProject ?? false;

	const [setupScript, setSetupScript] = useState("");
	const [runScript, setRunScript] = useState("");
	const [archiveScript, setArchiveScript] = useState("");
	const [autoRunSetup, setAutoRunSetup] = useState(false);
	const initialized = useRef(false);

	useEffect(() => {
		if (!data) return;
		const shouldSyncSetup = setupLocked || !initialized.current;
		const shouldSyncRun = runLocked || !initialized.current;
		const shouldSyncArchive = archiveLocked || !initialized.current;
		if (shouldSyncSetup) setSetupScript(data.setupScript ?? "");
		if (shouldSyncRun) setRunScript(data.runScript ?? "");
		if (shouldSyncArchive) setArchiveScript(data.archiveScript ?? "");
		if (!initialized.current) setAutoRunSetup(data.autoRunSetup);
		if (!setupLocked && !runLocked && !archiveLocked) {
			initialized.current = true;
		}
	}, [data, setupLocked, runLocked, archiveLocked]);

	// Reset when switching repos.
	useEffect(() => {
		initialized.current = false;
	}, [repoId]);

	const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

	const save = useCallback(
		(nextSetup: string, nextRun: string, nextArchive: string) => {
			if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
			saveTimerRef.current = setTimeout(() => {
				void updateRepoScripts(
					repoId,
					nextSetup.trim() || null,
					nextRun.trim() || null,
					nextArchive.trim() || null,
				).then(() => {
					void queryClient.invalidateQueries({
						queryKey: ["repoScripts", repoId],
					});
				});
			}, 600);
		},
		[repoId, queryClient],
	);

	const handleSetupChange = useCallback(
		(e: React.ChangeEvent<HTMLTextAreaElement>) => {
			const value = e.target.value;
			setSetupScript(value);
			save(value, runScript, archiveScript);
		},
		[runScript, archiveScript, save],
	);

	const handleRunChange = useCallback(
		(e: React.ChangeEvent<HTMLTextAreaElement>) => {
			const value = e.target.value;
			setRunScript(value);
			save(setupScript, value, archiveScript);
		},
		[setupScript, archiveScript, save],
	);

	const handleArchiveChange = useCallback(
		(e: React.ChangeEvent<HTMLTextAreaElement>) => {
			const value = e.target.value;
			setArchiveScript(value);
			save(setupScript, runScript, value);
		},
		[setupScript, runScript, save],
	);

	const handleAutoRunSetupChange = useCallback(
		(checked: boolean) => {
			setAutoRunSetup(checked);
			void updateRepoAutoRunSetup(repoId, checked).then(() => {
				void queryClient.invalidateQueries({
					queryKey: ["repoScripts", repoId],
				});
			});
		},
		[repoId, queryClient],
	);

	const setupHasScript = !!setupScript.trim();

	return (
		<div className="py-5">
			<div className="text-[13px] font-medium leading-snug text-foreground">
				Scripts
			</div>
			<div className="mt-1 text-[12px] leading-snug text-muted-foreground">
				Commands that run when workspaces are set up, run, or archived.
			</div>

			<div className="mt-4 space-y-4">
				<ScriptField
					label="Setup script"
					description="Available from the Setup tab in any workspace"
					placeholder="e.g., npm install"
					value={setupScript}
					locked={setupLocked}
					lockedMessage="Set by this workspace's winthorpe.json — edit it there"
					onChange={handleSetupChange}
					headerRight={
						<div className="flex items-center gap-1.5">
							<span className="text-[11px] font-medium text-muted-foreground">
								Auto-run
							</span>
							<TooltipProvider>
								<Tooltip>
									<TooltipTrigger asChild>
										<HelpCircle
											className="size-3 cursor-help text-muted-foreground/70"
											strokeWidth={1.8}
										/>
									</TooltipTrigger>
									<TooltipContent side="top" className="max-w-[240px]">
										On by default — setup runs automatically as soon as a
										workspace is created. Turn off to run it manually from the
										Setup tab.
									</TooltipContent>
								</Tooltip>
							</TooltipProvider>
							<Switch
								checked={autoRunSetup}
								onCheckedChange={handleAutoRunSetupChange}
								disabled={!setupHasScript}
								aria-label="Auto-run setup script on workspace creation"
							/>
						</div>
					}
				/>
				<ScriptField
					label="Run script"
					description="Runs when you click the play button"
					placeholder="e.g., npm run dev"
					value={runScript}
					locked={runLocked}
					lockedMessage="Set by this workspace's winthorpe.json — edit it there"
					onChange={handleRunChange}
				/>
				<ScriptField
					label="Archive script"
					description="Runs when a workspace is archived"
					placeholder="e.g., docker compose down"
					value={archiveScript}
					locked={archiveLocked}
					lockedMessage="Set by this workspace's winthorpe.json — edit it there"
					onChange={handleArchiveChange}
				/>
			</div>
		</div>
	);
}

function DeleteRepoSection({
	repo,
	onDeleted,
}: {
	repo: RepositoryCreateOption;
	onDeleted: () => void;
}) {
	const [confirmOpen, setConfirmOpen] = useState(false);
	const [deleting, setDeleting] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const handleDelete = useCallback(async () => {
		setDeleting(true);
		setError(null);
		try {
			await deleteRepository(repo.id);
			setConfirmOpen(false);
			onDeleted();
		} catch (e) {
			setError(errorMessage(e));
			setDeleting(false);
		}
	}, [repo.id, onDeleted]);

	return (
		<>
			<div className="py-5">
				<div className="flex items-center gap-2 text-[13px] font-medium leading-snug text-foreground">
					<Trash2 className="size-3.5 text-destructive" strokeWidth={1.8} />
					Delete Repository
				</div>
				<div className="mt-1 text-[12px] leading-snug text-muted-foreground">
					Permanently remove this repository and all its workspaces, sessions,
					and messages.
				</div>
				<Button
					variant="destructive"
					size="sm"
					className="mt-3"
					onClick={() => {
						setError(null);
						setConfirmOpen(true);
					}}
				>
					Delete Repository
				</Button>
				{error && (
					<div className="mt-3 rounded-lg border border-destructive/20 bg-destructive/5 px-3 py-2 text-[12px] text-destructive">
						{error}
					</div>
				)}
			</div>

			<ConfirmDialog
				open={confirmOpen}
				onOpenChange={setConfirmOpen}
				title={`Delete ${repo.name}?`}
				description={
					<>
						This will permanently delete all workspaces, sessions, and messages
						associated with{" "}
						<strong className="text-foreground/80">{repo.name}</strong>. This
						cannot be undone.
					</>
				}
				confirmLabel={deleting ? "Deleting..." : "Delete"}
				onConfirm={() => void handleDelete()}
				loading={deleting}
			/>
		</>
	);
}
