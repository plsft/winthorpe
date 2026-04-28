import { MarkGithubIcon } from "@primer/octicons-react";
import { ArrowLeft, ArrowRight, LogIn } from "lucide-react";
import {
	type ReactNode,
	useCallback,
	useEffect,
	useRef,
	useState,
} from "react";
import { toast } from "sonner";
import { GitlabBrandIcon } from "@/components/brand-icon";
import type { TerminalHandle } from "@/components/terminal-output";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
	type ForgeCliStatus,
	type ForgeProvider,
	getForgeCliStatus,
	resizeForgeCliAuthTerminal,
	type ScriptEvent,
	spawnForgeCliAuthTerminal,
	stopForgeCliAuthTerminal,
	writeForgeCliAuthTerminalStdin,
} from "@/lib/api";
import { cn } from "@/lib/utils";
import { OnboardingTerminalPreview } from "../components/login-terminal-preview";
import { SetupItem } from "../components/setup-item";
import type { OnboardingStep } from "../types";

const CLI_AUTH_POLL_INTERVAL_MS = 2000;
const CLI_AUTH_POLL_TIMEOUT_MS = 120_000;
const DEFAULT_GITLAB_HOST = "gitlab.com";

type RepoCliProvider = Exclude<ForgeProvider, "unknown">;
type GitlabPanel = "host" | null;

type ActiveTerminal = {
	provider: RepoCliProvider;
	host: string;
	instanceId: string;
};

type CliState = {
	status: ForgeCliStatus | null;
	checking: boolean;
};

export function RepositoryCliStep({
	step,
	onBack,
	onNext,
}: {
	step: OnboardingStep;
	onBack: () => void;
	onNext: () => void;
}) {
	const [github, setGithub] = useState<CliState>({
		status: null,
		checking: true,
	});
	const [gitlab, setGitlab] = useState<CliState>({
		status: null,
		checking: true,
	});
	const [gitlabHost, setGitlabHost] = useState(DEFAULT_GITLAB_HOST);
	const [gitlabStatusHost, setGitlabStatusHost] = useState(DEFAULT_GITLAB_HOST);
	const [activeGitlabPanel, setActiveGitlabPanel] = useState<GitlabPanel>(null);
	const [activeTerminal, setActiveTerminal] = useState<ActiveTerminal | null>(
		null,
	);
	const [waitingProvider, setWaitingProvider] =
		useState<RepoCliProvider | null>(null);
	const pollTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

	const clearPoll = useCallback(() => {
		if (pollTimerRef.current !== null) {
			clearTimeout(pollTimerRef.current);
			pollTimerRef.current = null;
		}
	}, []);

	const refreshStatus = useCallback(
		async (provider: RepoCliProvider, host: string) => {
			const next = await getForgeCliStatus(provider, host);
			if (provider === "github") {
				setGithub({ status: next, checking: false });
			} else {
				setGitlab({ status: next, checking: false });
			}
			return next;
		},
		[],
	);

	useEffect(() => {
		let cancelled = false;
		const load = async (
			provider: RepoCliProvider,
			host: string,
			setState: (state: CliState) => void,
		) => {
			setState({ status: null, checking: true });
			try {
				const status = await getForgeCliStatus(provider, host);
				if (!cancelled) setState({ status, checking: false });
			} catch (error) {
				if (!cancelled) {
					setState({
						status: {
							status: "error",
							provider,
							host,
							cliName: provider === "gitlab" ? "glab" : "gh",
							message: error instanceof Error ? error.message : String(error),
						},
						checking: false,
					});
				}
			}
		};
		void load("github", "github.com", setGithub);
		void load("gitlab", gitlabStatusHost, setGitlab);
		return () => {
			cancelled = true;
		};
	}, [gitlabStatusHost]);

	useEffect(() => clearPoll, [clearPoll]);

	const pollUntilReady = useCallback(
		(provider: RepoCliProvider, host: string, startedAt = Date.now()) => {
			clearPoll();
			pollTimerRef.current = setTimeout(async () => {
				try {
					const next = await refreshStatus(provider, host);
					if (next.status === "ready") {
						setWaitingProvider(null);
						toast.success(`${next.cliName} connected`);
						return;
					}
				} catch {
					// Auth may still be in progress in the embedded terminal.
				}
				if (Date.now() - startedAt >= CLI_AUTH_POLL_TIMEOUT_MS) {
					setWaitingProvider(null);
					toast(
						`Finish ${provider === "gitlab" ? "GitLab" : "GitHub"} CLI auth, then click Set up again.`,
					);
					return;
				}
				pollUntilReady(provider, host, startedAt);
			}, CLI_AUTH_POLL_INTERVAL_MS);
		},
		[clearPoll, refreshStatus],
	);

	const openTerminal = useCallback(
		(provider: RepoCliProvider, host: string) => {
			clearPoll();
			setWaitingProvider(provider);
			setActiveGitlabPanel(null);
			setActiveTerminal({
				provider,
				host,
				instanceId: crypto.randomUUID(),
			});
		},
		[clearPoll],
	);

	const handleTerminalExit = useCallback(
		(code: number | null) => {
			if (!activeTerminal) return;
			void refreshStatus(activeTerminal.provider, activeTerminal.host);
			if (code !== 0) {
				setWaitingProvider(null);
				return;
			}
			pollUntilReady(activeTerminal.provider, activeTerminal.host);
		},
		[activeTerminal, pollUntilReady, refreshStatus],
	);

	const handleTerminalError = useCallback(() => {
		setWaitingProvider(null);
	}, []);

	const handleGithubSetUp = useCallback(() => {
		if (github.status?.status === "ready") return;
		openTerminal("github", "github.com");
	}, [github.status, openTerminal]);

	const handleGitlabSetUp = useCallback(() => {
		clearPoll();
		setWaitingProvider(null);
		setActiveTerminal(null);
		setActiveGitlabPanel("host");
	}, [clearPoll]);

	const handleGitlabHostSubmit = useCallback(async () => {
		const host = normalizeGitlabHost(gitlabHost);
		if (!host) {
			toast.error("Enter a GitLab domain.");
			return;
		}
		setGitlabHost(host);
		setGitlabStatusHost(host);
		// Probe `glab auth status --hostname <host>` before spawning the terminal —
		// the domain may already be authenticated, in which case we skip straight
		// to the ready state instead of forcing the user through `glab auth login`.
		clearPoll();
		setWaitingProvider("gitlab");
		setGitlab((prev) => ({ status: prev.status, checking: true }));
		try {
			const status = await refreshStatus("gitlab", host);
			if (status.status === "ready") {
				setWaitingProvider(null);
				setActiveGitlabPanel(null);
				toast.success(`${status.cliName} connected`);
				return;
			}
		} catch {
			// Fall through to the terminal so the user can finish auth manually.
		}
		openTerminal("gitlab", host);
	}, [clearPoll, gitlabHost, openTerminal, refreshStatus]);

	return (
		<section
			aria-label="Repository CLI setup"
			aria-hidden={step !== "corner"}
			className={`absolute top-20 right-20 z-30 w-[560px] transition-all duration-1000 ease-[cubic-bezier(.22,.82,.2,1)] ${
				step === "skills"
					? "pointer-events-none translate-x-[118vw] -translate-y-[55vh] opacity-100"
					: step === "corner"
						? "translate-x-0 translate-y-0 opacity-100"
						: "pointer-events-none translate-x-[64vw] -translate-y-[108vh] opacity-100"
			}`}
		>
			<div className="flex flex-col items-start">
				<h2 className="max-w-none text-4xl font-semibold leading-[1.02] tracking-normal text-foreground whitespace-nowrap">
					Set up repository CLIs
				</h2>
				<p className="mt-4 max-w-md text-sm leading-6 text-muted-foreground">
					Install and authenticate your GitHub or GitLab CLI so Winthorpe can open
					pull requests and keep repository actions local.
				</p>

				<div className="mt-7 grid w-full gap-3">
					<RepositoryCliSetupItem
						icon={<MarkGithubIcon size={20} />}
						label="GitHub CLI"
						description="Run gh auth login to connect GitHub locally."
						status={github.status}
						checking={github.checking}
						waiting={waitingProvider === "github"}
						onSetUp={handleGithubSetUp}
					/>

					<RepositoryCliTerminalSlot
						active={activeTerminal?.provider === "github"}
						terminal={
							activeTerminal?.provider === "github" ? activeTerminal : null
						}
						onTerminalExit={handleTerminalExit}
						onTerminalError={handleTerminalError}
					/>

					<RepositoryCliSetupItem
						icon={<GitlabBrandIcon size={20} className="text-[#FC6D26]" />}
						label="GitLab CLI"
						description="Run glab auth login to connect GitLab locally."
						status={gitlab.status}
						checking={gitlab.checking}
						waiting={waitingProvider === "gitlab"}
						onSetUp={handleGitlabSetUp}
					/>

					<RepositoryCliGitlabPanel
						activePanel={activeGitlabPanel}
						activeTerminal={
							activeTerminal?.provider === "gitlab" ? activeTerminal : null
						}
						gitlabHost={gitlabHost}
						onGitlabHostChange={setGitlabHost}
						onGitlabHostSubmit={handleGitlabHostSubmit}
						onTerminalExit={handleTerminalExit}
						onTerminalError={handleTerminalError}
					/>
				</div>

				<div className="mt-7 flex items-center gap-3">
					<Button
						type="button"
						variant="ghost"
						size="lg"
						onClick={onBack}
						className="h-11 gap-2 px-4 text-[0.95rem]"
					>
						<ArrowLeft data-icon="inline-start" className="size-4" />
						Back
					</Button>
					<Button
						type="button"
						size="lg"
						onClick={onNext}
						className="h-11 gap-2 px-4 text-[0.95rem]"
					>
						Next
						<ArrowRight data-icon="inline-end" className="size-4" />
					</Button>
				</div>
			</div>
		</section>
	);
}

function RepositoryCliSetupItem({
	icon,
	label,
	description,
	status,
	checking,
	waiting,
	onSetUp,
}: {
	icon: ReactNode;
	label: string;
	description: string;
	status: ForgeCliStatus | null;
	checking: boolean;
	waiting: boolean;
	onSetUp: () => void;
}) {
	const ready = status?.status === "ready";
	const readyLogin = ready ? status.login.trim() : "";
	const displayLabel = readyLogin ? `${label} (${readyLogin})` : label;

	return (
		<SetupItem
			icon={icon}
			label={displayLabel}
			description={description}
			actionLabel={checking ? "Checking" : waiting ? "Waiting" : "Set up"}
			onAction={onSetUp}
			busy={checking || waiting}
			ready={ready}
		/>
	);
}

function RepositoryCliTerminalSlot({
	active,
	terminal,
	onTerminalExit,
	onTerminalError,
}: {
	active: boolean;
	terminal: ActiveTerminal | null;
	onTerminalExit: (code: number | null) => void;
	onTerminalError: (message: string) => void;
}) {
	return (
		<div
			className={cn(
				"overflow-hidden transition-all duration-700 ease-[cubic-bezier(.22,.82,.2,1)]",
				active ? "h-[282px]" : "h-0",
			)}
		>
			<div className="relative h-[270px] pt-3">
				<ForgeCliTerminalPreview
					active={active}
					terminal={terminal}
					onExit={onTerminalExit}
					onError={onTerminalError}
				/>
			</div>
		</div>
	);
}

function RepositoryCliGitlabPanel({
	activePanel,
	activeTerminal,
	gitlabHost,
	onGitlabHostChange,
	onGitlabHostSubmit,
	onTerminalExit,
	onTerminalError,
}: {
	activePanel: GitlabPanel;
	activeTerminal: ActiveTerminal | null;
	gitlabHost: string;
	onGitlabHostChange: (host: string) => void;
	onGitlabHostSubmit: () => void;
	onTerminalExit: (code: number | null) => void;
	onTerminalError: (message: string) => void;
}) {
	const isHostInput = activePanel === "host";
	const isTerminal = activeTerminal !== null;
	const isOpen = isHostInput || isTerminal;

	return (
		<div
			className={cn(
				"overflow-hidden transition-all duration-700 ease-[cubic-bezier(.22,.82,.2,1)]",
				isOpen ? "h-[282px]" : "h-0",
			)}
		>
			<div className="relative h-[270px] pt-3">
				<GitlabHostPanel
					active={isHostInput}
					value={gitlabHost}
					onChange={onGitlabHostChange}
					onSubmit={onGitlabHostSubmit}
				/>
				<ForgeCliTerminalPreview
					active={isTerminal}
					terminal={activeTerminal}
					onExit={onTerminalExit}
					onError={onTerminalError}
				/>
			</div>
		</div>
	);
}

function GitlabHostPanel({
	active,
	value,
	onChange,
	onSubmit,
}: {
	active: boolean;
	value: string;
	onChange: (value: string) => void;
	onSubmit: () => void;
}) {
	return (
		<div
			className={cn(
				"absolute inset-x-0 top-3 rounded-xl border border-border/55 bg-card p-4 shadow-2xl shadow-black/10 transition-all duration-700 ease-[cubic-bezier(.22,.82,.2,1)]",
				active
					? "translate-x-0 opacity-100"
					: "pointer-events-none translate-x-[calc(100%+3rem)] opacity-0",
			)}
		>
			<div className="text-sm font-medium text-foreground">GitLab domain</div>
			<p className="mt-1 text-xs leading-5 text-muted-foreground">
				Use gitlab.com or your self-hosted GitLab domain.
			</p>
			<form
				className="mt-4 flex items-center gap-2"
				onSubmit={(event) => {
					event.preventDefault();
					onSubmit();
				}}
			>
				<Input
					value={value}
					onChange={(event) => onChange(event.target.value)}
					placeholder={DEFAULT_GITLAB_HOST}
					aria-label="GitLab domain"
					className="h-10"
				/>
				<Button type="submit" className="h-10 shrink-0 gap-2 px-3">
					<LogIn className="size-4" />
					Log in
				</Button>
			</form>
		</div>
	);
}

function ForgeCliTerminalPreview({
	active,
	terminal,
	onExit,
	onError,
}: {
	active: boolean;
	terminal: ActiveTerminal | null;
	onExit: (code: number | null) => void;
	onError: (message: string) => void;
}) {
	const termRef = useRef<TerminalHandle | null>(null);

	useEffect(() => {
		if (!active || !terminal) return;

		let cancelled = false;
		const replay = () => {
			termRef.current?.clear();
			termRef.current?.refit();
		};

		if (termRef.current) replay();
		else requestAnimationFrame(replay);

		void spawnForgeCliAuthTerminal(
			terminal.provider,
			terminal.host,
			terminal.instanceId,
			(event: ScriptEvent) => {
				if (cancelled) return;
				switch (event.type) {
					case "stdout":
					case "stderr":
						termRef.current?.write(event.data);
						break;
					case "error":
						termRef.current?.write(`\r\n${event.message}\r\n`);
						onError(event.message);
						break;
					case "exited":
						onExit(event.code);
						break;
					case "started":
						break;
				}
			},
		).catch((error) => {
			if (cancelled) return;
			const message =
				error instanceof Error ? error.message : "Unable to start login.";
			termRef.current?.write(`\r\n${message}\r\n`);
			onError(message);
		});

		return () => {
			cancelled = true;
			void stopForgeCliAuthTerminal(
				terminal.provider,
				terminal.host,
				terminal.instanceId,
			);
		};
	}, [active, terminal, onExit, onError]);

	const handleData = useCallback(
		(data: string) => {
			if (!terminal) return;
			void writeForgeCliAuthTerminalStdin(
				terminal.provider,
				terminal.host,
				terminal.instanceId,
				data,
			);
		},
		[terminal],
	);

	const handleResize = useCallback(
		(cols: number, rows: number) => {
			if (!terminal) return;
			void resizeForgeCliAuthTerminal(
				terminal.provider,
				terminal.host,
				terminal.instanceId,
				cols,
				rows,
			);
		},
		[terminal],
	);

	if (!terminal) return null;

	const title =
		terminal.provider === "gitlab"
			? `GitLab CLI login · ${terminal.host}`
			: "GitHub CLI login";

	return (
		<OnboardingTerminalPreview
			title={title}
			active={active}
			terminalRef={termRef}
			heightClassName="h-[258px]"
			terminalClassName="h-[218px]"
			panelClassName="shadow-none"
			className="!relative !top-auto !right-auto !w-full !translate-y-0"
			onData={handleData}
			onResize={handleResize}
		/>
	);
}

function normalizeGitlabHost(value: string) {
	return value
		.trim()
		.replace(/^https?:\/\//i, "")
		.split("/")[0]
		.trim();
}
