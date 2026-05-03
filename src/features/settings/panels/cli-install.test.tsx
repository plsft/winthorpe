import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const apiMocks = vi.hoisted(() => ({
	getCliStatus: vi.fn(),
	installCli: vi.fn(),
}));

vi.mock("@/lib/api", async (importOriginal) => {
	const actual = await importOriginal<typeof import("@/lib/api")>();
	return {
		...actual,
		getCliStatus: apiMocks.getCliStatus,
		installCli: apiMocks.installCli,
	};
});

import { CliInstallPanel } from "./cli-install";

describe("CliInstallPanel", () => {
	beforeEach(() => {
		apiMocks.getCliStatus.mockReset();
		apiMocks.installCli.mockReset();
	});

	afterEach(() => {
		cleanup();
		vi.clearAllMocks();
	});

	it("renders the managed install state", async () => {
		apiMocks.getCliStatus.mockResolvedValue({
			installed: true,
			installPath: "/usr/local/bin/winthorpe-dev",
			buildMode: "development",
			installState: "managed",
		});

		render(<CliInstallPanel />);

		await waitFor(() => {
			expect(screen.getByText(/Installed at/)).toBeInTheDocument();
		});
		expect(screen.getByText("winthorpe-dev")).toBeInTheDocument();
		expect(
			screen.getByText("/usr/local/bin/winthorpe-dev"),
		).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Reinstall" }),
		).toBeInTheDocument();
	});

	it("renders the stale install state and allows reinstall", async () => {
		const user = userEvent.setup();
		apiMocks.getCliStatus.mockResolvedValue({
			installed: true,
			installPath: "/usr/local/bin/winthorpe",
			buildMode: "release",
			installState: "stale",
		});
		apiMocks.installCli.mockResolvedValue({
			installed: true,
			installPath: "/usr/local/bin/winthorpe",
			buildMode: "release",
			installState: "managed",
		});

		render(<CliInstallPanel />);

		await waitFor(() => {
			expect(
				screen.getByText(/is not managed by this app/i),
			).toBeInTheDocument();
		});

		await user.click(screen.getByRole("button", { name: "Reinstall" }));

		await waitFor(() => {
			expect(apiMocks.installCli).toHaveBeenCalledTimes(1);
		});
		await waitFor(() => {
			expect(screen.getByText(/Installed at/)).toBeInTheDocument();
		});
	});
});
