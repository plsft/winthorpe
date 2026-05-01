import { Eraser, Filter, Pause, Play, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
	clearLogEvents,
	getLogEvents,
	type LogEvent,
	type LogLevel,
} from "@/lib/api";
import { cn, errorMessage } from "@/lib/utils";

const POLL_INTERVAL_MS = 750;
const PAGE_SIZE = 500;
const MAX_RETAINED = 5000;

const LEVEL_RANK: Record<LogLevel, number> = {
	TRACE: 0,
	DEBUG: 1,
	INFO: 2,
	WARN: 3,
	ERROR: 4,
};

const LEVEL_LABEL: Record<LogLevel, string> = {
	TRACE: "trace",
	DEBUG: "debug",
	INFO: "info",
	WARN: "warn",
	ERROR: "error",
};

// Color tokens chosen to read well on both light and dark backgrounds via
// the app's existing semantic color system.
const LEVEL_PILL_CLASS: Record<LogLevel, string> = {
	TRACE: "text-muted-foreground/80 bg-muted/40",
	DEBUG: "text-muted-foreground bg-muted/60",
	INFO: "text-sky-700 bg-sky-100 dark:text-sky-200 dark:bg-sky-900/40",
	WARN: "text-amber-700 bg-amber-100 dark:text-amber-200 dark:bg-amber-900/40",
	ERROR: "text-rose-700 bg-rose-100 dark:text-rose-200 dark:bg-rose-900/40",
};

type Props = {
	open: boolean;
	onClose: () => void;
};

/**
 * Detailed activity log. Shows the last few thousand tracing events from
 * Rust — git invocations, sidecar IPC, agent CLI lifecycle, command
 * dispatch, errors. Polls every ~750 ms while open.
 *
 * Polling beats event subscription here because:
 *  - Backpressure is trivial — the frontend paces itself.
 *  - Reconnect after a stall is a no-op (next poll catches up via `since`).
 *  - The Tauri command also serves the initial backfill in one round-trip.
 */
export function ActivityLogDialog({ open, onClose }: Props) {
	const [events, setEvents] = useState<LogEvent[]>([]);
	const [paused, setPaused] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [minLevel, setMinLevel] = useState<LogLevel>("DEBUG");
	const [filterText, setFilterText] = useState("");
	const sinceRef = useRef(0);
	const scrollRef = useRef<HTMLDivElement | null>(null);
	const userScrolledUpRef = useRef(false);

	// Reset state every time the dialog re-opens so a stale buffer from a
	// previous open doesn't leak in.
	useEffect(() => {
		if (!open) return;
		sinceRef.current = 0;
		setEvents([]);
		setError(null);
	}, [open]);

	// Polling loop. Pauses when paused or when dialog is closed.
	useEffect(() => {
		if (!open || paused) return;

		let cancelled = false;

		const poll = async () => {
			try {
				const page = await getLogEvents(sinceRef.current, PAGE_SIZE);
				if (cancelled) return;
				if (page.events.length > 0) {
					sinceRef.current = page.nextSince;
					setEvents((prev) => {
						const next = prev.concat(page.events);
						return next.length > MAX_RETAINED
							? next.slice(next.length - MAX_RETAINED)
							: next;
					});
				}
				setError(null);
				// If the buffer signaled more pages, don't wait for the
				// regular interval — chase the tail immediately.
				if (page.hasMore && !cancelled) {
					queueMicrotask(poll);
				}
			} catch (e) {
				if (!cancelled) setError(errorMessage(e));
			}
		};

		void poll();
		const handle = window.setInterval(() => {
			void poll();
		}, POLL_INTERVAL_MS);
		return () => {
			cancelled = true;
			window.clearInterval(handle);
		};
	}, [open, paused]);

	// Auto-scroll to bottom unless the user has manually scrolled up. Toggling
	// pause re-enables auto-scroll.
	useEffect(() => {
		if (!open || paused) return;
		if (userScrolledUpRef.current) return;
		const node = scrollRef.current;
		if (!node) return;
		node.scrollTop = node.scrollHeight;
	}, [events, open, paused]);

	const handleScroll = useCallback(() => {
		const node = scrollRef.current;
		if (!node) return;
		const distanceFromBottom =
			node.scrollHeight - node.scrollTop - node.clientHeight;
		userScrolledUpRef.current = distanceFromBottom > 24;
	}, []);

	const handleClear = useCallback(async () => {
		try {
			await clearLogEvents();
			setEvents([]);
			sinceRef.current = 0;
		} catch (e) {
			setError(errorMessage(e));
		}
	}, []);

	const filtered = useMemo(() => {
		const minRank = LEVEL_RANK[minLevel];
		const needle = filterText.trim().toLowerCase();
		return events.filter((e) => {
			if (LEVEL_RANK[e.level] < minRank) return false;
			if (!needle) return true;
			return (
				e.message.toLowerCase().includes(needle) ||
				e.target.toLowerCase().includes(needle) ||
				e.fields.toLowerCase().includes(needle)
			);
		});
	}, [events, minLevel, filterText]);

	if (!open) return null;

	return (
		<div
			role="dialog"
			aria-modal="true"
			aria-label="Activity log"
			className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm"
			onClick={onClose}
			onKeyDown={(event) => {
				if (event.key === "Escape") onClose();
			}}
		>
			{/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
			<div
				className="flex h-[80vh] w-full max-w-5xl flex-col overflow-hidden rounded-xl border border-border bg-card shadow-2xl"
				onClick={(event) => event.stopPropagation()}
			>
				<header className="flex shrink-0 items-center justify-between border-b border-border/60 px-4 py-3">
					<div className="flex items-center gap-2">
						<h2 className="text-sm font-semibold text-foreground">
							Activity Log
						</h2>
						<span className="text-xs text-muted-foreground">
							{filtered.length} of {events.length} events
							{paused ? " · paused" : ""}
						</span>
					</div>
					<div className="flex items-center gap-1">
						<Button
							variant="ghost"
							size="sm"
							onClick={() => setPaused((p) => !p)}
							title={paused ? "Resume tail" : "Pause tail"}
						>
							{paused ? (
								<Play className="size-4" />
							) : (
								<Pause className="size-4" />
							)}
						</Button>
						<Button
							variant="ghost"
							size="sm"
							onClick={handleClear}
							title="Clear log buffer"
						>
							<Eraser className="size-4" />
						</Button>
						<Button
							variant="ghost"
							size="sm"
							onClick={onClose}
							aria-label="Close activity log"
						>
							<X className="size-4" />
						</Button>
					</div>
				</header>

				<div className="flex shrink-0 items-center gap-2 border-b border-border/60 bg-muted/30 px-4 py-2">
					<Filter className="size-3.5 text-muted-foreground" />
					<div className="flex items-center gap-1 text-xs">
						{(Object.keys(LEVEL_RANK) as LogLevel[]).map((level) => (
							<button
								key={level}
								type="button"
								onClick={() => setMinLevel(level)}
								className={cn(
									"rounded px-2 py-0.5 font-mono uppercase transition-colors",
									minLevel === level
										? "bg-foreground text-background"
										: "text-muted-foreground hover:bg-muted",
								)}
								title={`Show ${LEVEL_LABEL[level]} and above`}
							>
								{LEVEL_LABEL[level]}
							</button>
						))}
					</div>
					<div className="flex-1" />
					<Input
						value={filterText}
						onChange={(event) => setFilterText(event.target.value)}
						placeholder="Filter (target, message, fields)…"
						className="h-7 w-72 text-xs"
					/>
				</div>

				{error ? (
					<div className="shrink-0 border-b border-rose-300/60 bg-rose-50 px-4 py-2 text-xs text-rose-700 dark:border-rose-800/60 dark:bg-rose-950/40 dark:text-rose-200">
						{error}
					</div>
				) : null}

				{/* eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions */}
				<div
					ref={scrollRef}
					onScroll={handleScroll}
					className="flex-1 overflow-y-auto bg-background font-mono text-[11px] leading-snug"
				>
					{filtered.length === 0 ? (
						<div className="flex h-full items-center justify-center text-xs text-muted-foreground">
							{events.length === 0
								? "Waiting for activity…"
								: "No events match the current filter."}
						</div>
					) : (
						<ol className="divide-y divide-border/40">
							{filtered.map((entry) => (
								<LogRow key={entry.seq} entry={entry} />
							))}
						</ol>
					)}
				</div>

				<footer className="shrink-0 border-t border-border/60 bg-muted/30 px-4 py-2 text-[11px] text-muted-foreground">
					Logs auto-tail every {POLL_INTERVAL_MS} ms while open. Buffer caps at
					the most recent 5,000 Rust tracing events; the full history is also
					written to <span className="font-mono">logs/rust.jsonl</span> in your
					Winthorpe data directory.
				</footer>
			</div>
		</div>
	);
}

function LogRow({ entry }: { entry: LogEvent }) {
	const time = new Date(entry.timestampMs).toLocaleTimeString([], {
		hour12: false,
		hour: "2-digit",
		minute: "2-digit",
		second: "2-digit",
		// Note: fractionalSecondDigits is widely supported in modern Chromium.
		fractionalSecondDigits: 3,
	} as Intl.DateTimeFormatOptions);

	const targetShort = entry.target.replace(/^winthorpe(_lib)?::/, "");

	return (
		<li className="grid grid-cols-[auto_auto_auto_1fr] gap-3 px-4 py-1.5 hover:bg-muted/40">
			<span className="text-muted-foreground tabular-nums">{time}</span>
			<span
				className={cn(
					"rounded px-1.5 text-center text-[10px] font-semibold uppercase tracking-wide",
					LEVEL_PILL_CLASS[entry.level],
				)}
			>
				{LEVEL_LABEL[entry.level]}
			</span>
			<span className="text-muted-foreground" title={entry.target}>
				{targetShort}
			</span>
			<div className="min-w-0">
				<div className="break-words text-foreground">{entry.message}</div>
				{entry.fields ? (
					<div className="break-words text-muted-foreground">
						{entry.fields}
					</div>
				) : null}
			</div>
		</li>
	);
}
