import type { QueryClient } from "@tanstack/react-query";
import { winthorpeQueryKeys } from "./query-client";

// Module-level counter: any code path about to mutate the sidebar lists
// (archive, restore, create, delete, pin) wraps the async work in
// begin/endSidebarMutation. While the counter is non-zero, concurrent
// callers (mark-session-read, etc.) must NOT force a refetch of
// workspaceGroups / archivedWorkspaces — the backend state is still
// mid-transition and a refetch would overwrite the optimistic cache with
// a stale snapshot, causing the row to flicker back to its pre-mutation
// position before settling.
let pending = 0;

export function beginSidebarMutation(): void {
	pending += 1;
}

export function endSidebarMutation(): void {
	pending = Math.max(0, pending - 1);
}

export function isSidebarMutationInFlight(): boolean {
	return pending > 0;
}

/** Unconditional flush — used by the owner of the mutation when it
 * completes (counter has hit 0) and wants the server to reconcile. */
export function flushSidebarLists(queryClient: QueryClient): void {
	void queryClient.invalidateQueries({
		queryKey: winthorpeQueryKeys.workspaceGroups,
	});
	void queryClient.invalidateQueries({
		queryKey: winthorpeQueryKeys.archivedWorkspaces,
	});
}

/** Gated flush — safe for concurrent callers (mark-read, etc.) that want
 * to reconcile the sidebar only when no mutation is mid-flight. */
export function flushSidebarListsIfIdle(queryClient: QueryClient): void {
	if (pending > 0) return;
	flushSidebarLists(queryClient);
}

/** Test-only: reset the counter between test cases so leaked mutations
 * from one test don't gate flushes in the next. */
export function resetSidebarMutationGate(): void {
	pending = 0;
}
