import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { completeWorkspaceSetup } from "@/lib/api";
import { winthorpeQueryKeys } from "@/lib/query-client";
import { getScriptState, startScript } from "../script-store";

type UseSetupAutoRunArgs = {
	repoId: string | null;
	workspaceId: string | null;
	workspaceState: string | null;
	setupScript: string | null;
	scriptsLoaded: boolean;
};

/**
 * Runs setup-script auto-trigger / auto-complete logic at the inspector
 * sidebar level — independent of whether the Setup tab is mounted. The
 * script-store is module-scoped, so triggering startScript here seamlessly
 * hands output off to SetupTab if/when the user opens the tab.
 */
export function useSetupAutoRun({
	repoId,
	workspaceId,
	workspaceState,
	setupScript,
	scriptsLoaded,
}: UseSetupAutoRunArgs) {
	const queryClient = useQueryClient();
	const hasScript = !!setupScript?.trim();

	// Auto-run setup when workspace is pending and a script is configured.
	useEffect(() => {
		if (
			workspaceState !== "setup_pending" ||
			!hasScript ||
			!repoId ||
			!workspaceId
		) {
			return;
		}
		// Module-level script-store dedup: if we've already started/finished
		// a setup run for this workspace, don't trigger again.
		if (getScriptState(workspaceId, "setup")) {
			return;
		}
		startScript(repoId, "setup", workspaceId);
	}, [workspaceState, hasScript, repoId, workspaceId]);

	// Auto-complete if workspace is pending but no script is configured.
	useEffect(() => {
		if (
			workspaceState !== "setup_pending" ||
			!scriptsLoaded ||
			hasScript ||
			!workspaceId
		) {
			return;
		}
		void completeWorkspaceSetup(workspaceId).then(() => {
			queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceDetail(workspaceId),
			});
		});
	}, [workspaceState, scriptsLoaded, hasScript, workspaceId, queryClient]);
}
