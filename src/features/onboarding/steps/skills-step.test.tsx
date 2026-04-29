import {
	cleanup,
	render,
	screen,
	waitFor,
	within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const apiMocks = vi.hoisted(() => ({
	getCliStatus: vi.fn(),
	getWinthorpeSkillsStatus: vi.fn(),
	installCli: vi.fn(),
	installWinthorpeSkills: vi.fn(),
}));

vi.mock("@/lib/api", async (importOriginal) => {
	const actual = await importOriginal<typeof import("@/lib/api")>();
	return {
		...actual,
		getCliStatus: apiMocks.getCliStatus,
		getWinthorpeSkillsStatus: apiMocks.getWinthorpeSkillsStatus,
		installCli: apiMocks.installCli,
		installWinthorpeSkills: apiMocks.installWinthorpeSkills,
	};
});

vi.mock("sonner", () => ({
	toast: vi.fn(),
}));

import { SkillsStep } from "./skills-step";

describe("SkillsStep", () => {
	beforeEach(() => {
		apiMocks.getCliStatus.mockReset();
		apiMocks.getWinthorpeSkillsStatus.mockReset();
		apiMocks.installCli.mockReset();
		apiMocks.installWinthorpeSkills.mockReset();
		apiMocks.getWinthorpeSkillsStatus.mockResolvedValue({
			installed: false,
			claude: false,
			codex: false,
			command:
				"npx --yes skills add plsft/winthorpe/.codex/skills/winthorpe-cli -g -s winthorpe-cli -y --copy -a claude-code -a codex",
		});
	});

	afterEach(() => {
		cleanup();
		vi.clearAllMocks();
	});

	it("shows Ready when the Winthorpe CLI is already installed", async () => {
		apiMocks.getCliStatus.mockResolvedValue({
			installed: true,
			installPath: "/usr/local/bin/winthorpe-dev",
			buildMode: "development",
			installState: "managed",
		});

		render(
			<SkillsStep
				step="skills"
				onBack={vi.fn()}
				onNext={vi.fn()}
				isRoutingImport={false}
			/>,
		);

		const cliItem = screen.getByRole("group", { name: "Winthorpe CLI" });

		await waitFor(() => {
			expect(within(cliItem).getByText("Ready")).toBeInTheDocument();
		});
		expect(
			within(cliItem).queryByRole("button", { name: "Set up" }),
		).not.toBeInTheDocument();
		expect(apiMocks.installCli).not.toHaveBeenCalled();
	});

	it("installs the Winthorpe CLI from the setup item", async () => {
		const user = userEvent.setup();
		apiMocks.getCliStatus.mockResolvedValue({
			installed: false,
			installPath: null,
			buildMode: "development",
			installState: "missing",
		});
		apiMocks.installCli.mockResolvedValue({
			installed: true,
			installPath: "/usr/local/bin/winthorpe-dev",
			buildMode: "development",
			installState: "managed",
		});

		render(
			<SkillsStep
				step="skills"
				onBack={vi.fn()}
				onNext={vi.fn()}
				isRoutingImport={false}
			/>,
		);

		const cliItem = screen.getByRole("group", { name: "Winthorpe CLI" });

		await user.click(within(cliItem).getByRole("button", { name: "Set up" }));

		await waitFor(() => {
			expect(apiMocks.installCli).toHaveBeenCalledTimes(1);
		});
		expect(within(cliItem).getByText("Ready")).toBeInTheDocument();
		expect(
			within(cliItem).queryByRole("button", { name: "Set up" }),
		).not.toBeInTheDocument();
	});

	it("installs Winthorpe skills from the setup item", async () => {
		const user = userEvent.setup();
		apiMocks.getCliStatus.mockResolvedValue({
			installed: true,
			installPath: "/usr/local/bin/winthorpe-dev",
			buildMode: "development",
			installState: "managed",
		});
		apiMocks.installWinthorpeSkills.mockResolvedValue({
			installed: true,
			claude: true,
			codex: false,
			command:
				"npx --yes skills add plsft/winthorpe/.codex/skills/winthorpe-cli -g -s winthorpe-cli -y --copy -a claude-code",
		});

		render(
			<SkillsStep
				step="skills"
				onBack={vi.fn()}
				onNext={vi.fn()}
				isRoutingImport={false}
			/>,
		);

		const skillsItem = screen.getByRole("group", {
			name: "Winthorpe Skills (Beta)",
		});

		await user.click(
			within(skillsItem).getByRole("button", { name: "Set up" }),
		);

		await waitFor(() => {
			expect(apiMocks.installWinthorpeSkills).toHaveBeenCalledTimes(1);
		});
		expect(within(skillsItem).getByText("Ready")).toBeInTheDocument();
	});

	it("shows the unified failure hint when skills setup throws", async () => {
		const user = userEvent.setup();
		apiMocks.getCliStatus.mockResolvedValue({
			installed: true,
			installPath: "/usr/local/bin/winthorpe-dev",
			buildMode: "development",
			installState: "managed",
		});
		apiMocks.installWinthorpeSkills.mockRejectedValue(
			new Error("Winthorpe skills setup failed with a long stack trace."),
		);

		render(
			<SkillsStep
				step="skills"
				onBack={vi.fn()}
				onNext={vi.fn()}
				isRoutingImport={false}
			/>,
		);

		const skillsItem = screen.getByRole("group", {
			name: "Winthorpe Skills (Beta)",
		});

		await user.click(
			within(skillsItem).getByRole("button", { name: "Set up" }),
		);

		await waitFor(() => {
			expect(
				within(skillsItem).getByText(/something went wrong/i),
			).toBeInTheDocument();
		});
		expect(within(skillsItem).getByText(/don't worry/i)).toBeInTheDocument();
		expect(
			within(skillsItem).queryByText(/long stack trace/i),
		).not.toBeInTheDocument();
	});
});
