import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import {
	type ForgeCliStatus,
	type ForgeProvider,
	openForgeCliAuthTerminal,
} from "@/lib/api";
import {
	forgeCliStatusQueryOptions,
	winthorpeQueryKeys,
} from "@/lib/query-client";

const POLL_INTERVAL_MS = 2000;
const POLL_TIMEOUT_MS = 120_000;

type ForgeCliReadyStatus = Extract<ForgeCliStatus, { status: "ready" }>;

export type UseForgeCliConnectOptions = {
	// Fired the first time polling observes `status === "ready"`. Caller can
	// chain extra invalidation here.
	onReady?: (status: ForgeCliReadyStatus) => void | Promise<void>;
	// Skip the default `"<cliName> connected"` toast.
	silent?: boolean;
	// If already `ready`, skip the terminal hand-off and fan out directly.
	hintedStatus?: ForgeCliStatus | null;
};

// Shared open-terminal + poll-until-ready flow. On ready we invalidate both
// the `forgeCliStatus` cache and the `workspaceForge` cache that embeds it,
// so the two layers can't disagree.
export function useForgeCliConnect(
	provider: ForgeProvider,
	host: string,
	options: UseForgeCliConnectOptions = {},
) {
	const queryClient = useQueryClient();
	const [connecting, setConnecting] = useState(false);
	const pollTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
	const inFlightRef = useRef(false);
	const onReadyRef = useRef(options.onReady);
	const silentRef = useRef(options.silent ?? false);
	const hintedStatusRef = useRef(options.hintedStatus ?? null);

	useEffect(() => {
		onReadyRef.current = options.onReady;
		silentRef.current = options.silent ?? false;
		hintedStatusRef.current = options.hintedStatus ?? null;
	}, [options.hintedStatus, options.onReady, options.silent]);

	useEffect(() => {
		return () => {
			if (pollTimerRef.current) clearTimeout(pollTimerRef.current);
		};
	}, []);

	const finishReady = useCallback(
		async (status: ForgeCliReadyStatus) => {
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.forgeCliStatusAll,
			});
			void queryClient.invalidateQueries({
				predicate: (q) => q.queryKey[0] === "workspaceForge",
			});
			if (!silentRef.current) {
				toast.success(`${status.cliName} connected`);
			}
			await onReadyRef.current?.(status);
			setConnecting(false);
			inFlightRef.current = false;
		},
		[queryClient],
	);

	const pollUntilReady = useCallback(
		(startedAt = Date.now()) => {
			if (pollTimerRef.current) clearTimeout(pollTimerRef.current);
			pollTimerRef.current = setTimeout(async () => {
				try {
					const next = await queryClient.fetchQuery(
						forgeCliStatusQueryOptions(provider, host),
					);
					if (next.status === "ready") {
						await finishReady(next);
						return;
					}
					if (next.status === "error") {
						// Persistent error (CLI not installed, exec failure, etc.).
						// Sitting silent until the 120s budget burns is the wrong UX —
						// surface it now so the user knows the terminal won't help.
						toast.error(next.message);
						setConnecting(false);
						inFlightRef.current = false;
						return;
					}
				} catch {
					// Transient IPC error — keep polling, auth may still be in progress.
				}
				if (Date.now() - startedAt >= POLL_TIMEOUT_MS) {
					toast("Finish CLI auth in Terminal, then click Connect again.");
					setConnecting(false);
					inFlightRef.current = false;
					return;
				}
				pollUntilReady(startedAt);
			}, POLL_INTERVAL_MS);
		},
		[finishReady, host, provider, queryClient],
	);

	const connect = useCallback(async () => {
		if (connecting || inFlightRef.current) return;
		inFlightRef.current = true;
		setConnecting(true);
		try {
			// Caller-provided hint or cached `ready` short-circuits the terminal
			// hand-off — e.g. user already authed via another surface and we just
			// need to fan out invalidations.
			const hint = hintedStatusRef.current;
			const cached =
				hint ??
				queryClient.getQueryData<ForgeCliStatus>(
					winthorpeQueryKeys.forgeCliStatus(provider, host),
				);
			if (cached?.status === "ready") {
				await finishReady(cached);
				return;
			}
			await openForgeCliAuthTerminal(provider, host);
			toast("Complete the login in Terminal.");
			pollUntilReady();
		} catch (error) {
			toast.error(
				error instanceof Error ? error.message : "Failed to open Terminal.",
			);
			setConnecting(false);
			inFlightRef.current = false;
		}
	}, [connecting, finishReady, host, pollUntilReady, provider, queryClient]);

	return { connect, connecting };
}
