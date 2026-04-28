import { QueryClient } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UiMutationEvent } from "@/lib/api";
import { winthorpeQueryKeys } from "@/lib/query-client";
import { useUiSyncBridge } from "./use-ui-sync-bridge";

const apiMocks = vi.hoisted(() => ({
	subscribeUiMutations: vi.fn(),
}));

let capturedSubscription: ((event: UiMutationEvent) => void) | null = null;

vi.mock("@/lib/api", async () => {
	const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
	return {
		...actual,
		subscribeUiMutations: apiMocks.subscribeUiMutations.mockImplementation(
			async (callback: (event: UiMutationEvent) => void) => {
				capturedSubscription = callback;
			},
		),
	};
});

function makeClient() {
	return new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
}

describe("useUiSyncBridge", () => {
	beforeEach(() => {
		capturedSubscription = null;
		apiMocks.subscribeUiMutations.mockClear();
	});

	it("invalidates the expected query families for workspace git state changes", async () => {
		const queryClient = makeClient();
		const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");

		renderHook(() =>
			useUiSyncBridge({
				queryClient,
				processPendingCliSends: vi.fn(),
				reloadSettings: vi.fn(),
				refreshGithubIdentity: vi.fn(),
			}),
		);

		expect(apiMocks.subscribeUiMutations).toHaveBeenCalledOnce();
		expect(capturedSubscription).not.toBeNull();

		act(() => {
			capturedSubscription?.({
				type: "workspaceGitStateChanged",
				workspaceId: "workspace-1",
			});
		});

		await waitFor(() => {
			expect(invalidateQueries).toHaveBeenCalledWith({
				queryKey: winthorpeQueryKeys.workspaceGroups,
			});
			expect(invalidateQueries).toHaveBeenCalledWith({
				queryKey: winthorpeQueryKeys.workspaceDetail("workspace-1"),
			});
			expect(invalidateQueries).toHaveBeenCalledWith({
				queryKey: winthorpeQueryKeys.workspaceGitActionStatus("workspace-1"),
			});
			expect(invalidateQueries).toHaveBeenCalledWith({
				queryKey: winthorpeQueryKeys.workspaceForgeActionStatus("workspace-1"),
			});
			expect(invalidateQueries).toHaveBeenCalledWith({
				predicate: expect.any(Function),
			});
		});
	});

	it("replays pending CLI sends immediately instead of waiting for focus", async () => {
		const queryClient = makeClient();
		const processPendingCliSends = vi.fn();

		renderHook(() =>
			useUiSyncBridge({
				queryClient,
				processPendingCliSends,
				reloadSettings: vi.fn(),
				refreshGithubIdentity: vi.fn(),
			}),
		);

		act(() => {
			capturedSubscription?.({
				type: "pendingCliSendQueued",
				workspaceId: "workspace-1",
				sessionId: "session-1",
				prompt: "hello",
				modelId: "gpt-5.4",
				permissionMode: "default",
			});
		});

		await waitFor(() => {
			expect(processPendingCliSends).toHaveBeenCalledOnce();
		});
	});

	it("invalidates forge detection when forge state changes", async () => {
		const queryClient = makeClient();
		const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");

		renderHook(() =>
			useUiSyncBridge({
				queryClient,
				processPendingCliSends: vi.fn(),
				reloadSettings: vi.fn(),
				refreshGithubIdentity: vi.fn(),
			}),
		);

		act(() => {
			capturedSubscription?.({
				type: "workspaceForgeChanged",
				workspaceId: "workspace-1",
			});
		});

		await waitFor(() => {
			expect(invalidateQueries).toHaveBeenCalledWith({
				queryKey: winthorpeQueryKeys.workspaceForge("workspace-1"),
			});
		});
		// Settings → Account stores CLI auth under a separate cache key; the
		// bridge fans the same backend signal out to it so a stale "ready"
		// in Account can't survive an auth flip detected elsewhere.
		expect(invalidateQueries).toHaveBeenCalledWith({
			queryKey: winthorpeQueryKeys.forgeCliStatusAll,
		});
	});

	it("invalidates baseline + rich on contextUsageChanged", async () => {
		const queryClient = makeClient();
		const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");

		renderHook(() =>
			useUiSyncBridge({
				queryClient,
				processPendingCliSends: vi.fn(),
				reloadSettings: vi.fn(),
				refreshGithubIdentity: vi.fn(),
			}),
		);

		act(() => {
			capturedSubscription?.({
				type: "contextUsageChanged",
				sessionId: "session-7",
			});
		});

		await waitFor(() => {
			expect(invalidateQueries).toHaveBeenCalledWith({
				queryKey: winthorpeQueryKeys.sessionContextUsage("session-7"),
			});
		});
		// And a predicate-based invalidate for rich entries scoped to
		// this session (any providerSessionId / model).
		expect(invalidateQueries).toHaveBeenCalledWith(
			expect.objectContaining({ predicate: expect.any(Function) }),
		);
		expect(invalidateQueries).toHaveBeenCalledTimes(2);
	});

	it("reloads settings and refreshes auto-close queries on settings changes", async () => {
		const queryClient = makeClient();
		const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
		const reloadSettings = vi.fn();

		renderHook(() =>
			useUiSyncBridge({
				queryClient,
				processPendingCliSends: vi.fn(),
				reloadSettings,
				refreshGithubIdentity: vi.fn(),
			}),
		);

		act(() => {
			capturedSubscription?.({
				type: "settingsChanged",
				key: "auto_close_action_kinds",
			});
		});

		await waitFor(() => {
			expect(reloadSettings).not.toHaveBeenCalled();
			expect(invalidateQueries).toHaveBeenCalledWith({
				queryKey: winthorpeQueryKeys.autoCloseActionKinds,
			});
			expect(invalidateQueries).toHaveBeenCalledWith({
				queryKey: winthorpeQueryKeys.autoCloseOptInAsked,
			});
		});

		act(() => {
			capturedSubscription?.({
				type: "settingsChanged",
				key: "app.default_model_id",
			});
		});

		await waitFor(() => {
			expect(reloadSettings).toHaveBeenCalledOnce();
		});
	});
});
