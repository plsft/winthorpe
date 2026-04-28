import { QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { createWinthorpeQueryClient, winthorpeQueryKeys } from "@/lib/query-client";
import { DEFAULT_SETTINGS, SettingsContext } from "@/lib/settings";
import { useEnsureDefaultModel } from "./use-ensure-default-model";

function renderUseEnsureDefaultModel(args: {
	defaultModelId: string | null;
	sections: Array<{
		id: "claude" | "codex";
		label: string;
		status?: "ready" | "unavailable" | "error";
		options: Array<{
			id: string;
			provider: "claude" | "codex";
			label: string;
			cliModel: string;
		}>;
	}>;
}) {
	const queryClient = createWinthorpeQueryClient();
	queryClient.setQueryData(winthorpeQueryKeys.agentModelSections, args.sections);
	const updateSettings = vi.fn();

	const wrapper = ({ children }: { children: ReactNode }) => (
		<QueryClientProvider client={queryClient}>
			<SettingsContext.Provider
				value={{
					settings: {
						...DEFAULT_SETTINGS,
						defaultModelId: args.defaultModelId,
					},
					isLoaded: true,
					updateSettings,
				}}
			>
				{children}
			</SettingsContext.Provider>
		</QueryClientProvider>
	);

	renderHook(() => useEnsureDefaultModel(), { wrapper });
	return { updateSettings };
}

describe("useEnsureDefaultModel", () => {
	it("repairs an invalid saved model once the catalog is settled", () => {
		const { updateSettings } = renderUseEnsureDefaultModel({
			defaultModelId: "gpt-legacy",
			sections: [
				{
					id: "claude",
					label: "Claude Code",
					status: "ready",
					options: [
						{
							id: "opus-1m",
							provider: "claude",
							label: "Opus",
							cliModel: "opus-1m",
						},
					],
				},
				{
					id: "codex",
					label: "Codex",
					status: "unavailable",
					options: [],
				},
			],
		});

		expect(updateSettings).toHaveBeenCalledWith({ defaultModelId: "opus-1m" });
	});

	it("preserves an invalid saved model while any provider is still in error", () => {
		const { updateSettings } = renderUseEnsureDefaultModel({
			defaultModelId: "gpt-legacy",
			sections: [
				{
					id: "claude",
					label: "Claude Code",
					status: "ready",
					options: [
						{
							id: "opus-1m",
							provider: "claude",
							label: "Opus",
							cliModel: "opus-1m",
						},
					],
				},
				{
					id: "codex",
					label: "Codex",
					status: "error",
					options: [],
				},
			],
		});

		expect(updateSettings).not.toHaveBeenCalled();
	});
});
