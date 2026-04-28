import { useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import { useCallback } from "react";
import { GithubBrandIcon, GitlabBrandIcon } from "@/components/brand-icon";
import { Button } from "@/components/ui/button";
import {
	Tooltip,
	TooltipContent,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import type { ForgeDetection } from "@/lib/api";
import { FORGE_AUTH_TOOLTIP_LINES } from "@/lib/forge-auth-copy";
import { winthorpeQueryKeys } from "@/lib/query-client";
import { useForgeCliConnect } from "@/lib/use-forge-cli-connect";

export function ForgeCliTrigger({
	detection,
	workspaceId,
	authRequired,
}: {
	detection: ForgeDetection;
	workspaceId: string | null;
	authRequired?: boolean;
}) {
	const queryClient = useQueryClient();

	// Inspector-specific tail of the connect flow: once CLI auth flips to
	// ready, the PR + CI surfaces (which were gated on auth) need to refresh
	// for the *current* workspace. The hook already invalidates the broader
	// workspaceForge / forgeCliStatus caches.
	const refreshWorkspaceSurfaces = useCallback(async () => {
		if (!workspaceId) return;
		await Promise.all([
			queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceChangeRequest(workspaceId),
			}),
			queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceForgeActionStatus(workspaceId),
			}),
		]);
	}, [queryClient, workspaceId]);

	const { connect, connecting } = useForgeCliConnect(
		detection.provider,
		detection.host ?? "",
		{
			onReady: refreshWorkspaceSurfaces,
			// Only trust the prop's `ready` when remote agrees. If the remote
			// probe is what put us here (`authRequired`), the local CLI snapshot
			// is stale — short-circuiting would just toast "connected" while
			// nothing actually worked. Force the terminal hand-off instead.
			hintedStatus: authRequired ? null : (detection.cli ?? null),
		},
	);

	return (
		<div className="ml-auto flex items-center self-center translate-y-px">
			<Tooltip delayDuration={150}>
				<TooltipTrigger asChild>
					<Button
						type="button"
						size="xs"
						variant="default"
						onClick={() => void connect()}
						disabled={connecting}
						className="gap-1 bg-primary text-primary-foreground hover:bg-primary/90"
					>
						{connecting ? (
							<Loader2 className="size-3 animate-spin text-current" />
						) : detection.provider === "gitlab" ? (
							<GitlabBrandIcon
								size={12}
								className="self-center text-[#FC6D26]"
							/>
						) : (
							<GithubBrandIcon size={12} className="self-center" />
						)}
						{detection.labels.connectAction}
					</Button>
				</TooltipTrigger>
				<TooltipContent side="bottom" className="max-w-xs whitespace-normal">
					<ForgeDetectionTooltipBody
						detection={detection}
						authRequired={authRequired}
					/>
				</TooltipContent>
			</Tooltip>
		</div>
	);
}

function ForgeDetectionTooltipBody({
	detection,
	authRequired,
}: {
	detection: ForgeDetection;
	authRequired?: boolean;
}) {
	const providerName = detection.labels.providerName;
	const host = detection.host ?? "this host";
	const cliStatus = detection.cli;
	const showConnectCopy =
		cliStatus?.status === "unauthenticated" ||
		authRequired === true ||
		!cliStatus;

	return (
		<div className="space-y-1.5">
			<div className="text-[11px] font-medium leading-snug">
				Detected {providerName} at {host}
			</div>
			{showConnectCopy ? (
				<div className="space-y-0.5 text-[10.5px] leading-snug opacity-90">
					{FORGE_AUTH_TOOLTIP_LINES.map((line) => (
						<div key={line}>{line}</div>
					))}
				</div>
			) : cliStatus?.status === "ready" ? (
				<div className="text-[10.5px] leading-snug opacity-90">
					Connected as {cliStatus.login}.
				</div>
			) : null}
			{detection.detectionSignals.length > 0 && (
				<div className="space-y-0.5 border-t border-background/20 pt-1.5 text-[10.5px] leading-snug opacity-90">
					<div className="font-medium">Why we think so:</div>
					<ul className="list-disc space-y-0.5 pl-3.5">
						{detection.detectionSignals.map((signal) => (
							<li key={`${signal.layer}:${signal.detail}`}>{signal.detail}</li>
						))}
					</ul>
				</div>
			)}
		</div>
	);
}
