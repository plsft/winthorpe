import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ForgeCliStatus } from "@/lib/api";
import { winthorpeQueryKeys } from "@/lib/query-client";
import { useForgeCliConnect } from "./use-forge-cli-connect";

const apiMocks = vi.hoisted(() => ({
	getForgeCliStatus: vi.fn(),
	openForgeCliAuthTerminal: vi.fn(),
}));

vi.mock("@/lib/api", async (importOriginal) => {
	const actual = await importOriginal<typeof import("@/lib/api")>();
	return {
		...actual,
		getForgeCliStatus: apiMocks.getForgeCliStatus,
		openForgeCliAuthTerminal: apiMocks.openForgeCliAuthTerminal,
	};
});

const toastMocks = vi.hoisted(() => {
	const toast = vi.fn();
	const success = vi.fn();
	const error = vi.fn();
	const dismiss = vi.fn();
	Object.assign(toast, { success, error, dismiss });
	return { toast, success, error, dismiss };
});

vi.mock("sonner", () => ({
	toast: toastMocks.toast,
}));

const readyStatus: ForgeCliStatus = {
	status: "ready",
	provider: "github",
	host: "github.com",
	cliName: "gh",
	login: "natllian",
	version: "2.65.0",
	message: "GitHub CLI ready as natllian.",
};

const unauthStatus: ForgeCliStatus = {
	status: "unauthenticated",
	provider: "github",
	host: "github.com",
	cliName: "gh",
	version: "2.65.0",
	message: "Run `gh auth login`.",
	loginCommand: "gh auth login",
};

const errorStatus: ForgeCliStatus = {
	status: "error",
	provider: "github",
	host: "github.com",
	cliName: "gh",
	version: null,
	message: "gh CLI not found in PATH.",
};

function makeWrapper() {
	const client = new QueryClient({
		defaultOptions: { queries: { retry: false } },
	});
	const wrapper = ({ children }: { children: ReactNode }) => (
		<QueryClientProvider client={client}>{children}</QueryClientProvider>
	);
	return { client, wrapper };
}

describe("useForgeCliConnect", () => {
	beforeEach(() => {
		apiMocks.getForgeCliStatus.mockReset();
		apiMocks.openForgeCliAuthTerminal.mockReset();
		apiMocks.openForgeCliAuthTerminal.mockResolvedValue(undefined);
		toastMocks.toast.mockClear();
		toastMocks.success.mockClear();
		toastMocks.error.mockClear();
	});

	afterEach(() => {
		vi.useRealTimers();
	});

	it("opens the terminal, polls until ready, then fans out invalidations and fires onReady", async () => {
		vi.useFakeTimers();
		apiMocks.getForgeCliStatus.mockResolvedValue(readyStatus);
		const { client, wrapper } = makeWrapper();
		const invalidateSpy = vi.spyOn(client, "invalidateQueries");
		const onReady = vi.fn();

		const { result } = renderHook(
			() => useForgeCliConnect("github", "github.com", { onReady }),
			{ wrapper },
		);

		// `await connect()` resolves once the terminal hand-off finishes and
		// the first poll setTimeout is scheduled.
		await act(async () => {
			await result.current.connect();
		});

		expect(apiMocks.openForgeCliAuthTerminal).toHaveBeenCalledWith(
			"github",
			"github.com",
		);
		expect(result.current.connecting).toBe(true);

		// First poll tick lands on ready. `runOnlyPendingTimersAsync` fires
		// the queued 2s timer and drains the inner microtask chain so the
		// React state update lands inside `act`.
		await act(async () => {
			await vi.runOnlyPendingTimersAsync();
		});

		expect(apiMocks.getForgeCliStatus).toHaveBeenCalledWith(
			"github",
			"github.com",
		);
		// Hook fans out to BOTH cache layers.
		expect(invalidateSpy).toHaveBeenCalledWith({
			queryKey: winthorpeQueryKeys.forgeCliStatusAll,
		});
		expect(invalidateSpy).toHaveBeenCalledWith(
			expect.objectContaining({ predicate: expect.any(Function) }),
		);
		expect(onReady).toHaveBeenCalledWith(readyStatus);
		expect(toastMocks.success).toHaveBeenCalledWith("gh connected");
		expect(result.current.connecting).toBe(false);
	});

	it("short-circuits the terminal hand-off when the standalone cache already says ready", async () => {
		const { client, wrapper } = makeWrapper();
		client.setQueryData(
			winthorpeQueryKeys.forgeCliStatus("github", "github.com"),
			readyStatus,
		);
		const onReady = vi.fn();

		const { result } = renderHook(
			() => useForgeCliConnect("github", "github.com", { onReady }),
			{ wrapper },
		);

		await act(async () => {
			await result.current.connect();
		});

		expect(apiMocks.openForgeCliAuthTerminal).not.toHaveBeenCalled();
		expect(onReady).toHaveBeenCalledWith(readyStatus);
		expect(toastMocks.success).toHaveBeenCalledWith("gh connected");
	});

	it("short-circuits when the caller passes a `hintedStatus` that is already ready", async () => {
		const { wrapper } = makeWrapper();
		const onReady = vi.fn();

		const { result } = renderHook(
			() =>
				useForgeCliConnect("github", "github.com", {
					onReady,
					hintedStatus: readyStatus,
				}),
			{ wrapper },
		);

		await act(async () => {
			await result.current.connect();
		});

		expect(apiMocks.openForgeCliAuthTerminal).not.toHaveBeenCalled();
		expect(onReady).toHaveBeenCalledWith(readyStatus);
	});

	it("does not short-circuit when the hint is unauthenticated — proceeds with the terminal flow", async () => {
		vi.useFakeTimers();
		apiMocks.getForgeCliStatus.mockResolvedValue(unauthStatus);
		const { wrapper } = makeWrapper();

		const { result } = renderHook(
			() =>
				useForgeCliConnect("github", "github.com", {
					hintedStatus: unauthStatus,
				}),
			{ wrapper },
		);

		await act(async () => {
			await result.current.connect();
		});

		expect(apiMocks.openForgeCliAuthTerminal).toHaveBeenCalledOnce();
	});

	it("times out after the poll budget, leaving connecting=false and no onReady", async () => {
		vi.useFakeTimers();
		apiMocks.getForgeCliStatus.mockResolvedValue(unauthStatus);
		const { wrapper } = makeWrapper();
		const onReady = vi.fn();

		const { result } = renderHook(
			() => useForgeCliConnect("github", "github.com", { onReady }),
			{ wrapper },
		);

		await act(async () => {
			await result.current.connect();
		});

		// Burn the full 120s budget worth of 2s polls. Each pending timer
		// fires once and reschedules the next, so we step through them.
		for (let i = 0; i < 65; i++) {
			await act(async () => {
				await vi.runOnlyPendingTimersAsync();
			});
			if (!result.current.connecting) break;
		}

		expect(onReady).not.toHaveBeenCalled();
		expect(toastMocks.toast).toHaveBeenCalledWith(
			expect.stringContaining("Finish CLI auth"),
		);
		expect(result.current.connecting).toBe(false);
	});

	it("ignores re-entrant connect() calls while a flow is already in flight", async () => {
		vi.useFakeTimers();
		apiMocks.getForgeCliStatus.mockResolvedValue(unauthStatus);
		const { wrapper } = makeWrapper();

		const { result } = renderHook(
			() => useForgeCliConnect("github", "github.com"),
			{ wrapper },
		);

		await act(async () => {
			void result.current.connect();
			void result.current.connect();
			void result.current.connect();
		});

		expect(apiMocks.openForgeCliAuthTerminal).toHaveBeenCalledOnce();
	});

	it("respects `silent: true` and skips the default ready toast", async () => {
		vi.useFakeTimers();
		apiMocks.getForgeCliStatus.mockResolvedValue(readyStatus);
		const { wrapper } = makeWrapper();

		const { result } = renderHook(
			() => useForgeCliConnect("github", "github.com", { silent: true }),
			{ wrapper },
		);

		await act(async () => {
			await result.current.connect();
		});
		await act(async () => {
			await vi.runOnlyPendingTimersAsync();
		});

		expect(toastMocks.success).not.toHaveBeenCalled();
	});

	it("surfaces a CLI error status during polling instead of silently waiting out the timeout", async () => {
		vi.useFakeTimers();
		apiMocks.getForgeCliStatus.mockResolvedValue(errorStatus);
		const { wrapper } = makeWrapper();
		const onReady = vi.fn();

		const { result } = renderHook(
			() => useForgeCliConnect("github", "github.com", { onReady }),
			{ wrapper },
		);

		await act(async () => {
			await result.current.connect();
		});
		await act(async () => {
			await vi.runOnlyPendingTimersAsync();
		});

		expect(toastMocks.error).toHaveBeenCalledWith(errorStatus.message);
		expect(onReady).not.toHaveBeenCalled();
		expect(result.current.connecting).toBe(false);
		// One poll call, then we stopped — no second tick.
		expect(apiMocks.getForgeCliStatus).toHaveBeenCalledTimes(1);
	});

	it("clears its poll timer on unmount so callbacks don't fire after teardown", async () => {
		vi.useFakeTimers();
		apiMocks.getForgeCliStatus.mockResolvedValue(unauthStatus);
		const { wrapper } = makeWrapper();
		const onReady = vi.fn();

		const { result, unmount } = renderHook(
			() => useForgeCliConnect("github", "github.com", { onReady }),
			{ wrapper },
		);

		await act(async () => {
			await result.current.connect();
		});

		const callsBeforeUnmount = apiMocks.getForgeCliStatus.mock.calls.length;
		unmount();

		// The poll callback is queued; advance well past one tick. Without
		// the cleanup the next `getForgeCliStatus` would fire and onReady /
		// toasts would still trip after the consumer is gone.
		await vi.advanceTimersByTimeAsync(10_000);

		expect(apiMocks.getForgeCliStatus.mock.calls.length).toBe(
			callsBeforeUnmount,
		);
		expect(onReady).not.toHaveBeenCalled();
	});
});
