import { useQuery } from "@tanstack/react-query";
import { CircleDollarSign } from "lucide-react";
import { getLastPrCostForWorkspace } from "@/lib/api";
import { cn } from "@/lib/utils";

/**
 * Tiny "Last PR: $0.05" chip for the Inspector commit/PR header.
 *
 * Reads the most-recent PR-create session's cost for the workspace.
 * Returns null (renders nothing) when there's no PR-create turn yet,
 * so the header stays clean for new workspaces.
 *
 * The query is cheap — single SUM(cost_usd) read, so we let React Query
 * cache it and refetch when the workspace's session list changes.
 */
export function PrCostChip({
	workspaceId,
	className,
}: {
	workspaceId: string | null;
	className?: string;
}) {
	const { data: cost } = useQuery({
		queryKey: ["ai_sessions", "last_pr_cost", workspaceId],
		queryFn: () =>
			workspaceId ? getLastPrCostForWorkspace(workspaceId) : Promise.resolve(0),
		enabled: !!workspaceId,
		staleTime: 30_000,
	});

	if (!workspaceId || !cost || cost <= 0) return null;

	return (
		<span
			title="Cost of the most recent PR-create turn (sum across retries)"
			className={cn(
				"inline-flex items-center gap-1 rounded-md bg-emerald-500/10 px-1.5 py-0.5 text-[10px] font-medium text-emerald-300",
				className,
			)}
		>
			<CircleDollarSign className="size-3" />
			Last PR {formatUsd(cost)}
		</span>
	);
}

function formatUsd(value: number): string {
	if (value < 0.01) return `$${value.toFixed(4)}`;
	if (value < 1) return `$${value.toFixed(3)}`;
	return `$${value.toFixed(2)}`;
}
