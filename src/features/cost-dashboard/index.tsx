import { CircleDollarSign, RefreshCw, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
	type AiSession,
	type AiSessionStats,
	type ClaudePlanSummary,
	getAiSessionStats,
	getClaudePlanSummary,
	getClaudeRateLimits,
	listRecentAiSessions,
	resetAiSessionLedger,
} from "@/lib/api";
import { cn, errorMessage } from "@/lib/utils";

type Tab = "summary" | "sessions" | "history";

type Props = {
	open: boolean;
	onClose: () => void;
};

/**
 * Cost & Tokens dashboard. Three tabs:
 *   - Summary  → totals + cost-by-provider breakdown
 *   - Sessions → flat list of recent turns (one row per agent turn)
 *   - History  → day-by-day cost trend (bars, last 30 days)
 *
 * Schema mirrors worktale's `ai_sessions`. Future export to worktale is
 * a straight column copy. Reads happen on open + on a manual refresh
 * button — no polling, since cost data only changes on agent turn end.
 */
export function CostDashboard({ open, onClose }: Props) {
	const [tab, setTab] = useState<Tab>("summary");
	const [stats, setStats] = useState<AiSessionStats | null>(null);
	const [sessions, setSessions] = useState<AiSession[] | null>(null);
	const [planSummary, setPlanSummary] = useState<ClaudePlanSummary | null>(
		null,
	);
	const [planUsage, setPlanUsage] = useState<unknown | null>(null);
	const [loading, setLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const loadAll = useCallback(async () => {
		setLoading(true);
		setError(null);
		try {
			// Fire all four in parallel. Plan summary + rate limits both
			// can be null (no creds, no fetch yet) — that's fine; the
			// panel just shows the API-equivalent stats below it.
			const [s, recent, plan, limitsRaw] = await Promise.all([
				getAiSessionStats(),
				listRecentAiSessions(500),
				getClaudePlanSummary().catch(() => null),
				getClaudeRateLimits().catch(() => null),
			]);
			setStats(s);
			setSessions(recent);
			setPlanSummary(plan);
			if (limitsRaw) {
				try {
					setPlanUsage(JSON.parse(limitsRaw));
				} catch {
					setPlanUsage(null);
				}
			} else {
				setPlanUsage(null);
			}
		} catch (e) {
			setError(errorMessage(e));
		} finally {
			setLoading(false);
		}
	}, []);

	useEffect(() => {
		if (!open) return;
		void loadAll();
	}, [open, loadAll]);

	const handleReset = useCallback(async () => {
		// Two-step confirm: native window.confirm is the right primitive
		// here because the destructive action is *permanent* (rows are
		// deleted from SQLite and the processed-state file is removed).
		// A toast undo would lie about being recoverable.
		const ok = window.confirm(
			"Reset cost ledger?\n\n" +
				"This deletes every row in ai_sessions and clears the transcript-" +
				"processed state file so the next 60s scan re-evaluates whatever's " +
				"still on disk.\n\n" +
				"Your actual transcripts on disk are NOT touched — only Winthorpe's " +
				"index of them. To stop old sessions reappearing, delete or move the " +
				"transcript files at ~/.claude/projects/ and ~/.codex/sessions/ first.",
		);
		if (!ok) return;
		try {
			const deleted = await resetAiSessionLedger();
			toast.success(
				deleted === 0
					? "Ledger was already empty."
					: `Cleared ${deleted.toLocaleString()} session ${
							deleted === 1 ? "row" : "rows"
						}.`,
			);
			await loadAll();
		} catch (e) {
			setError(errorMessage(e));
			toast.error("Reset failed", { description: errorMessage(e) });
		}
	}, [loadAll]);

	if (!open) return null;

	return (
		<div
			role="dialog"
			aria-modal="true"
			aria-label="Cost dashboard"
			className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm"
			onClick={onClose}
			onKeyDown={(e) => {
				if (e.key === "Escape") onClose();
			}}
		>
			{/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
			<div
				className="flex h-[80vh] w-full max-w-4xl flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl"
				onClick={(e) => e.stopPropagation()}
			>
				<header className="flex shrink-0 items-center justify-between border-b border-border/60 px-4 py-3">
					<div className="flex items-center gap-2">
						<CircleDollarSign className="size-4 text-emerald-500" />
						<h2 className="text-sm font-semibold text-foreground">
							Cost &amp; Tokens{" "}
							<span className="text-[11px] font-normal text-muted-foreground">
								(API-equivalent)
							</span>
						</h2>
						{stats ? (
							<span className="text-xs text-muted-foreground">
								·{" "}
								<span className="font-mono text-foreground">
									{formatUsd(stats.totalCostUsd)}
								</span>{" "}
								across {stats.totalTurns.toLocaleString()} turns
							</span>
						) : null}
					</div>
					<div className="flex items-center gap-1">
						<Button
							variant="ghost"
							size="sm"
							onClick={() => void loadAll()}
							disabled={loading}
							title="Refresh"
						>
							<RefreshCw className={cn("size-4", loading && "animate-spin")} />
						</Button>
						<Button
							variant="ghost"
							size="sm"
							onClick={() => void handleReset()}
							disabled={loading}
							title="Reset cost ledger (destructive)"
							className="text-rose-300 hover:bg-rose-500/15 hover:text-rose-200"
						>
							<Trash2 className="size-4" />
						</Button>
						<Button
							variant="ghost"
							size="sm"
							onClick={onClose}
							aria-label="Close cost dashboard"
						>
							<X className="size-4" />
						</Button>
					</div>
				</header>

				<div className="flex shrink-0 items-center gap-1 border-b border-border/60 bg-muted/30 px-4 py-2">
					<TabButton
						active={tab === "summary"}
						onClick={() => setTab("summary")}
					>
						Summary
					</TabButton>
					<TabButton
						active={tab === "sessions"}
						onClick={() => setTab("sessions")}
					>
						Sessions
					</TabButton>
					<TabButton
						active={tab === "history"}
						onClick={() => setTab("history")}
					>
						History
					</TabButton>
				</div>

				{error ? (
					<div className="shrink-0 border-b border-rose-300/60 bg-rose-50 px-4 py-2 text-xs text-rose-700 dark:border-rose-800/60 dark:bg-rose-950/40 dark:text-rose-200">
						{error}
					</div>
				) : null}

				<div className="flex-1 overflow-y-auto bg-background">
					{loading && !stats ? (
						<EmptyState>Loading…</EmptyState>
					) : tab === "summary" ? (
						<SummaryView
							stats={stats}
							planSummary={planSummary}
							planUsage={planUsage}
						/>
					) : tab === "sessions" ? (
						<SessionsList sessions={sessions ?? []} />
					) : (
						<HistoryView sessions={sessions ?? []} />
					)}
				</div>

				<footer className="shrink-0 border-t border-border/60 bg-muted/30 px-4 py-2 text-[11px] leading-relaxed text-muted-foreground">
					Costs are <strong className="text-foreground">API-equivalent</strong>{" "}
					— what each turn would cost on the metered Anthropic / OpenAI APIs. If
					you're on a Claude or OpenAI <em>subscription</em> (Pro / Max /
					ChatGPT Plus), Anthropic / OpenAI bills the flat plan instead — this
					number is your "value extracted," not your invoice. Heavy sessions can
					run up large numbers because each tool round-trip re-reads the prompt
					cache, and every cache read is a separately billable event. Token
					snapshots come from the JSONL transcripts written by Claude Code (
					<span className="font-mono">~/.claude/projects/</span>) and Codex (
					<span className="font-mono">~/.codex/sessions/</span>). Schema mirrors{" "}
					<a
						href="https://github.com/worktale"
						target="_blank"
						rel="noreferrer"
						className="underline decoration-dotted hover:text-foreground"
					>
						worktale's ai_sessions
					</a>
					.
				</footer>
			</div>
		</div>
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
				"rounded-md px-3 py-1 text-xs font-medium transition-colors",
				active
					? "bg-foreground text-background"
					: "text-muted-foreground hover:bg-muted hover:text-foreground",
			)}
		>
			{children}
		</button>
	);
}

function EmptyState({ children }: { children: React.ReactNode }) {
	return (
		<div className="flex h-full items-center justify-center text-xs text-muted-foreground">
			{children}
		</div>
	);
}

function SummaryView({
	stats,
	planSummary,
	planUsage,
}: {
	stats: AiSessionStats | null;
	planSummary: ClaudePlanSummary | null;
	planUsage: unknown | null;
}) {
	if (!stats || stats.totalTurns === 0) {
		return (
			<div className="space-y-6 p-4">
				<PlanUsagePanel planSummary={planSummary} planUsage={planUsage} />
				<EmptyState>
					No turns recorded yet. Send a prompt to an agent — its cost will land
					here when the turn finishes.
				</EmptyState>
			</div>
		);
	}

	const cards: Array<{ label: string; value: string; sub?: string }> = [
		{
			label: "Total cost",
			value: formatUsd(stats.totalCostUsd),
			sub: `across ${stats.totalTurns.toLocaleString()} turns`,
		},
		{
			label: "Input tokens",
			value: formatTokens(stats.totalInputTokens),
		},
		{
			label: "Output tokens",
			value: formatTokens(stats.totalOutputTokens),
		},
		{
			label: "Cache savings",
			value: formatTokens(
				stats.totalCacheReadTokens + stats.totalCacheWriteTokens,
			),
			sub: `${formatTokens(stats.totalCacheReadTokens)} read · ${formatTokens(
				stats.totalCacheWriteTokens,
			)} write`,
		},
	];

	const totalProvider = stats.costByProvider.reduce(
		(sum, p) => sum + p.costUsd,
		0,
	);

	return (
		<div className="space-y-6 p-4">
			<PlanUsagePanel planSummary={planSummary} planUsage={planUsage} />
			<div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
				{cards.map((card) => (
					<div
						key={card.label}
						className="rounded-lg border border-border/60 bg-card p-4"
					>
						<div className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
							{card.label}
						</div>
						<div className="mt-1 font-mono text-2xl font-semibold tabular-nums text-foreground">
							{card.value}
						</div>
						{card.sub ? (
							<div className="mt-1 text-[11px] text-muted-foreground">
								{card.sub}
							</div>
						) : null}
					</div>
				))}
			</div>

			{stats.costByProvider.length > 0 ? (
				<section>
					<h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
						Cost by provider
					</h3>
					<ul className="space-y-2">
						{stats.costByProvider.map((p) => {
							const pct = totalProvider ? (p.costUsd / totalProvider) * 100 : 0;
							return (
								<li
									key={p.provider}
									className="rounded-md border border-border/60 bg-card px-3 py-2"
								>
									<div className="flex items-center justify-between text-xs">
										<span className="font-medium text-foreground">
											{prettyProvider(p.provider)}
										</span>
										<span className="font-mono tabular-nums text-foreground">
											{formatUsd(p.costUsd)}
										</span>
									</div>
									<div className="mt-1 flex items-center gap-2 text-[11px] text-muted-foreground">
										<div className="h-1.5 flex-1 overflow-hidden rounded bg-muted">
											<div
												className="h-full bg-emerald-500"
												style={{ width: `${pct.toFixed(1)}%` }}
											/>
										</div>
										<span className="font-mono tabular-nums">
											{pct.toFixed(1)}% · {p.turns} turns
										</span>
									</div>
								</li>
							);
						})}
					</ul>
				</section>
			) : null}
		</div>
	);
}

function SessionsList({ sessions }: { sessions: AiSession[] }) {
	if (sessions.length === 0) {
		return <EmptyState>No turns recorded yet.</EmptyState>;
	}
	return (
		<table className="w-full text-xs">
			<thead className="sticky top-0 bg-card text-[11px] uppercase tracking-wide text-muted-foreground">
				<tr className="border-b border-border/60">
					<th className="px-3 py-2 text-left">When</th>
					<th className="px-3 py-2 text-left">Provider · model</th>
					<th className="px-3 py-2 text-right">Input</th>
					<th className="px-3 py-2 text-right">Output</th>
					<th className="px-3 py-2 text-right">Cache</th>
					<th
						className="px-3 py-2 text-right"
						title="API-equivalent cost — your invoice depends on your billing plan"
					>
						API cost
					</th>
					<th className="px-3 py-2 text-left">Tags</th>
				</tr>
			</thead>
			<tbody>
				{sessions.map((s) => (
					<tr
						key={s.id}
						className="border-b border-border/30 transition-colors hover:bg-muted/40"
					>
						<td className="px-3 py-1.5 text-muted-foreground tabular-nums">
							{formatTime(s.timestamp)}
						</td>
						<td className="px-3 py-1.5">
							<span className="font-medium text-foreground">
								{prettyProvider(s.provider)}
							</span>
							{s.model ? (
								<span className="text-muted-foreground"> · {s.model}</span>
							) : null}
						</td>
						<td className="px-3 py-1.5 text-right font-mono tabular-nums text-foreground">
							{formatTokens(s.inputTokens)}
						</td>
						<td className="px-3 py-1.5 text-right font-mono tabular-nums text-foreground">
							{formatTokens(s.outputTokens)}
						</td>
						<td className="px-3 py-1.5 text-right font-mono tabular-nums text-muted-foreground">
							{formatTokens(s.cacheReadTokens + s.cacheWriteTokens)}
						</td>
						<td className="px-3 py-1.5 text-right font-mono tabular-nums text-foreground">
							{formatUsd(s.costUsd)}
						</td>
						<td className="px-3 py-1.5 text-[10px]">
							{s.isPrCreate ? (
								<span className="rounded bg-violet-500/15 px-1.5 py-0.5 font-medium uppercase text-violet-300">
									PR
								</span>
							) : null}
						</td>
					</tr>
				))}
			</tbody>
		</table>
	);
}

function HistoryView({ sessions }: { sessions: AiSession[] }) {
	const byDay = useMemo(() => {
		const map = new Map<string, { cost: number; turns: number }>();
		for (const s of sessions) {
			const cur = map.get(s.date) ?? { cost: 0, turns: 0 };
			cur.cost += s.costUsd;
			cur.turns += 1;
			map.set(s.date, cur);
		}
		const entries = Array.from(map.entries())
			.sort(([a], [b]) => (a < b ? 1 : -1))
			.slice(0, 30);
		return entries;
	}, [sessions]);

	if (byDay.length === 0) {
		return <EmptyState>No history yet.</EmptyState>;
	}

	const max = Math.max(...byDay.map(([, v]) => v.cost));

	return (
		<div className="p-4">
			<div className="mb-2 text-xs text-muted-foreground">
				Last {byDay.length} day{byDay.length === 1 ? "" : "s"} with activity
			</div>
			<ul className="space-y-1">
				{byDay.map(([date, value]) => {
					const pct = max > 0 ? (value.cost / max) * 100 : 0;
					return (
						<li
							key={date}
							className="grid grid-cols-[100px_1fr_70px_60px] items-center gap-3 px-2 py-1.5 text-xs"
						>
							<span className="font-mono text-muted-foreground tabular-nums">
								{date}
							</span>
							<div className="h-2 overflow-hidden rounded bg-muted">
								<div
									className="h-full bg-emerald-500"
									style={{ width: `${pct.toFixed(1)}%` }}
								/>
							</div>
							<span className="text-right font-mono tabular-nums text-foreground">
								{formatUsd(value.cost)}
							</span>
							<span className="text-right text-muted-foreground tabular-nums">
								{value.turns}
							</span>
						</li>
					);
				})}
			</ul>
		</div>
	);
}

/**
 * "Plan usage" panel — what your subscription actually meters (Claude Max
 * 5h + 7d windows, etc). Renders only when we have credentials. The plan
 * tier comes from the local credentials file (no network), the usage
 * percentages from Anthropic's `oauth/usage` endpoint via the existing
 * `getClaudeRateLimits` command. Both can be null at first paint — show
 * what we have, hide the rest.
 *
 * Anthropic's `oauth/usage` response shape isn't officially documented,
 * so this panel parses defensively and gracefully degrades. Fields we
 * know about today (snapshot 2025-Q4):
 *   - `five_hour.utilization` (0..1)
 *   - `seven_day.utilization` (0..1)
 *   - `seven_day_opus.utilization` (0..1)  — Opus sub-limit
 *   - `*.resets_at` ISO timestamps for each window
 * If a future API rev renames anything, the panel just stops showing
 * that row — never blows up.
 */
function PlanUsagePanel({
	planSummary,
	planUsage,
}: {
	planSummary: ClaudePlanSummary | null;
	planUsage: unknown;
}) {
	if (!planSummary && !planUsage) return null;

	const tier =
		planSummary?.subscriptionType?.trim() ||
		planSummary?.rateLimitTier?.trim() ||
		"unknown";
	const tierLabel =
		(
			{
				max: "Max",
				pro: "Pro",
				free: "Free",
				raw_api_key: "API key (pay-as-you-go)",
				default: "Default",
				unknown: "Unknown plan",
			} as Record<string, string>
		)[tier] ?? tier;

	const onSubscription = tier === "max" || tier === "pro";

	// Defensive read of the usage payload.
	const u = planUsage as Record<string, unknown> | null;
	const fiveHour = pickWindow(u, ["five_hour", "fiveHour"]);
	const sevenDay = pickWindow(u, ["seven_day", "sevenDay"]);
	const sevenDayOpus = pickWindow(u, ["seven_day_opus", "sevenDayOpus"]);

	const haveLiveUsage = !!(fiveHour || sevenDay || sevenDayOpus);

	return (
		<section className="rounded-lg border border-border/60 bg-card p-4">
			<header className="mb-3 flex items-center justify-between">
				<div className="flex items-center gap-2">
					<span className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
						Plan usage
					</span>
					<span
						className={cn(
							"rounded px-1.5 py-0.5 text-[10px] font-medium uppercase",
							onSubscription
								? "bg-emerald-500/15 text-emerald-300"
								: "bg-muted text-muted-foreground",
						)}
					>
						{tierLabel}
					</span>
				</div>
				{!onSubscription && (
					<span className="text-[11px] text-muted-foreground">
						Cost &amp; tokens below are your actual invoice
					</span>
				)}
				{onSubscription && (
					<span className="text-[11px] text-muted-foreground">
						Cost &amp; tokens below are API-equivalent (value extracted)
					</span>
				)}
			</header>

			{!haveLiveUsage && onSubscription ? (
				<p className="text-xs text-muted-foreground">
					Live quota numbers haven't loaded yet. Anthropic's usage endpoint is
					fetched on a throttle — refresh in a minute.
				</p>
			) : null}

			{haveLiveUsage ? (
				<div className="space-y-2">
					{fiveHour ? (
						<UsageBar
							label="5-hour window"
							utilization={fiveHour.utilization}
							resetsAt={fiveHour.resetsAt}
						/>
					) : null}
					{sevenDay ? (
						<UsageBar
							label="7-day window"
							utilization={sevenDay.utilization}
							resetsAt={sevenDay.resetsAt}
						/>
					) : null}
					{sevenDayOpus ? (
						<UsageBar
							label="Opus 7-day sub-limit"
							utilization={sevenDayOpus.utilization}
							resetsAt={sevenDayOpus.resetsAt}
						/>
					) : null}
				</div>
			) : null}

			{planSummary && !planSummary.accessTokenValid ? (
				<p className="mt-2 text-[11px] text-amber-300">
					Local Claude token is expired — Winthorpe will trigger a refresh on
					the next agent turn or rate-limit fetch.
				</p>
			) : null}
		</section>
	);
}

type ParsedWindow = {
	utilization: number | null;
	resetsAt: string | null;
};

function pickWindow(
	payload: Record<string, unknown> | null,
	keys: string[],
): ParsedWindow | null {
	if (!payload) return null;
	for (const k of keys) {
		const v = payload[k];
		if (!v || typeof v !== "object") continue;
		const o = v as Record<string, unknown>;
		const utilRaw =
			(o.utilization as number | undefined) ??
			(o.percentage as number | undefined) ??
			null;
		const resetsAt =
			(o.resets_at as string | undefined) ??
			(o.resetsAt as string | undefined) ??
			null;
		if (typeof utilRaw === "number" || resetsAt) {
			return {
				utilization: typeof utilRaw === "number" ? utilRaw : null,
				resetsAt: resetsAt ?? null,
			};
		}
	}
	return null;
}

function UsageBar({
	label,
	utilization,
	resetsAt,
}: {
	label: string;
	utilization: number | null;
	resetsAt: string | null;
}) {
	// utilization is 0..1 in Anthropic's response.
	const pct =
		utilization === null ? null : Math.min(100, Math.max(0, utilization * 100));
	const reset = formatRelativeReset(resetsAt);
	const barColor =
		pct === null
			? "bg-muted-foreground/40"
			: pct >= 90
				? "bg-rose-500"
				: pct >= 70
					? "bg-amber-500"
					: "bg-emerald-500";
	return (
		<div className="text-xs">
			<div className="flex items-center justify-between">
				<span className="text-foreground">{label}</span>
				<span className="font-mono tabular-nums text-muted-foreground">
					{pct === null ? "—" : `${pct.toFixed(0)}%`}
					{reset ? ` · resets ${reset}` : ""}
				</span>
			</div>
			<div className="mt-1 h-1.5 overflow-hidden rounded bg-muted">
				<div
					className={cn("h-full transition-[width]", barColor)}
					style={{ width: `${pct ?? 0}%` }}
				/>
			</div>
		</div>
	);
}

function formatRelativeReset(iso: string | null): string {
	if (!iso) return "";
	const t = new Date(iso).getTime();
	if (Number.isNaN(t)) return "";
	const diffMs = t - Date.now();
	if (diffMs <= 0) return "now";
	const m = Math.round(diffMs / 60_000);
	if (m < 60) return `in ${m}m`;
	const h = Math.round(m / 60);
	if (h < 24) return `in ${h}h ${m % 60}m`;
	const d = Math.round(h / 24);
	return `in ${d}d`;
}

function formatUsd(value: number): string {
	if (value < 0.01 && value > 0) return `$${value.toFixed(4)}`;
	if (value < 1) return `$${value.toFixed(3)}`;
	return `$${value.toFixed(2)}`;
}

function formatTokens(value: number): string {
	if (value < 1_000) return value.toString();
	if (value < 1_000_000) return `${(value / 1_000).toFixed(1)}k`;
	return `${(value / 1_000_000).toFixed(2)}M`;
}

function formatTime(iso: string): string {
	const d = new Date(`${iso.replace(" ", "T")}Z`);
	if (Number.isNaN(d.getTime())) return iso;
	return d.toLocaleString([], {
		month: "short",
		day: "numeric",
		hour: "2-digit",
		minute: "2-digit",
		hour12: false,
	});
}

function prettyProvider(p: string | null): string {
	if (!p) return "—";
	if (p === "claude") return "Claude";
	if (p === "codex") return "Codex";
	return p;
}
