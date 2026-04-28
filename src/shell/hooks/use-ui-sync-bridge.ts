import type { QueryClient } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { subscribeUiMutations, type UiMutationEvent } from "@/lib/api";
import { winthorpeQueryKeys } from "@/lib/query-client";

type Options = {
	queryClient: QueryClient;
	processPendingCliSends: () => Promise<void> | void;
	reloadSettings: () => Promise<void> | void;
	refreshGithubIdentity: () => Promise<void> | void;
};

function invalidateAllWorkspaceChanges(queryClient: QueryClient) {
	void queryClient.invalidateQueries({
		predicate: (query) => query.queryKey[0] === "workspaceChanges",
	});
	void queryClient.invalidateQueries({
		predicate: (query) => query.queryKey[0] === "workspaceFiles",
	});
}

function handleUiMutation(
	event: UiMutationEvent,
	queryClient: QueryClient,
	options: Omit<Options, "queryClient">,
) {
	switch (event.type) {
		case "workspaceListChanged":
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceGroups,
			});
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.archivedWorkspaces,
			});
			void queryClient.invalidateQueries({
				predicate: (query) =>
					query.queryKey[0] === "workspaceCandidateDirectories",
			});
			return;
		case "workspaceChanged":
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceGroups,
			});
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceDetail(event.workspaceId),
			});
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceLinkedDirectories(event.workspaceId),
			});
			return;
		case "sessionListChanged":
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceGroups,
			});
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceDetail(event.workspaceId),
			});
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceSessions(event.workspaceId),
			});
			return;
		case "contextUsageChanged":
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.sessionContextUsage(event.sessionId),
			});
			void queryClient.invalidateQueries({
				predicate: (query) =>
					query.queryKey[0] === "claudeRichContextUsage" &&
					query.queryKey[1] === event.sessionId,
			});
			return;
		case "workspaceFilesChanged":
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceGitActionStatus(event.workspaceId),
			});
			invalidateAllWorkspaceChanges(queryClient);
			return;
		case "workspaceGitStateChanged":
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceGroups,
			});
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceDetail(event.workspaceId),
			});
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceGitActionStatus(event.workspaceId),
			});
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceForgeActionStatus(event.workspaceId),
			});
			invalidateAllWorkspaceChanges(queryClient);
			return;
		case "workspaceForgeChanged":
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceForge(event.workspaceId),
			});
			// CLI auth status lives in a separate cache (Settings → Account).
			// Backend already debounces/edge-detects this event, so the bridge
			// is the right place to fan out instead of redoing the check in
			// individual feature components.
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.forgeCliStatusAll,
			});
			return;
		case "workspaceChangeRequestChanged":
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceGroups,
			});
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceDetail(event.workspaceId),
			});
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceChangeRequest(event.workspaceId),
			});
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceForgeActionStatus(event.workspaceId),
			});
			return;
		case "repositoryListChanged":
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.repositories,
			});
			return;
		case "repositoryChanged":
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.repositories,
			});
			void queryClient.invalidateQueries({
				predicate: (query) =>
					query.queryKey[0] === "repoScripts" &&
					query.queryKey[1] === event.repoId,
			});
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.repoPreferences(event.repoId),
			});
			void queryClient.invalidateQueries({
				predicate: (query) => query.queryKey[0] === "workspaceDetail",
			});
			void queryClient.invalidateQueries({
				queryKey: winthorpeQueryKeys.workspaceGroups,
			});
			return;
		case "settingsChanged":
			if (
				event.key === null ||
				event.key.startsWith("app.") ||
				event.key.startsWith("branch_prefix_")
			) {
				void options.reloadSettings();
			}
			if (
				event.key === null ||
				event.key === "auto_close_action_kinds" ||
				event.key === "auto_close_opt_in_asked"
			) {
				void queryClient.invalidateQueries({
					queryKey: winthorpeQueryKeys.autoCloseActionKinds,
				});
				void queryClient.invalidateQueries({
					queryKey: winthorpeQueryKeys.autoCloseOptInAsked,
				});
			}
			return;
		case "githubIdentityChanged":
			void options.refreshGithubIdentity();
			return;
		case "pendingCliSendQueued":
			void options.processPendingCliSends();
			return;
	}
}

export function useUiSyncBridge({
	queryClient,
	processPendingCliSends,
	reloadSettings,
	refreshGithubIdentity,
}: Options) {
	const processPendingCliSendsRef = useRef(processPendingCliSends);
	const reloadSettingsRef = useRef(reloadSettings);
	const refreshGithubIdentityRef = useRef(refreshGithubIdentity);

	useEffect(() => {
		processPendingCliSendsRef.current = processPendingCliSends;
		reloadSettingsRef.current = reloadSettings;
		refreshGithubIdentityRef.current = refreshGithubIdentity;
	}, [processPendingCliSends, refreshGithubIdentity, reloadSettings]);

	useEffect(() => {
		let disposed = false;

		void subscribeUiMutations((event) => {
			if (disposed) {
				return;
			}

			handleUiMutation(event, queryClient, {
				processPendingCliSends: () => processPendingCliSendsRef.current(),
				reloadSettings: () => reloadSettingsRef.current(),
				refreshGithubIdentity: () => refreshGithubIdentityRef.current(),
			});
		});

		return () => {
			disposed = true;
		};
	}, [queryClient]);
}
