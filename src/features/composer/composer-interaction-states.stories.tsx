/**
 * Visual reference for the composer interaction surfaces:
 *
 *   - Default: Lexical editor + toolbar
 *   - State A: Permission request — adapted through the shared
 *              `GenericDeferredToolPanel` (same UI as State B)
 *   - State B: Deferred tool approval (GenericDeferredToolPanel)
 *   - State C: Elicitation (FormElicitationPanel / UrlElicitationPanel)
 *
 * The stories render the real production components with mocked prop
 * values — no JSX is reimplemented here. If the app's styling changes,
 * these previews change automatically.
 */

import type { Meta, StoryObj } from "@storybook/react-vite";
import { type QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { PendingPermission } from "@/features/conversation/hooks/use-streaming";
import type { PendingDeferredTool } from "@/features/conversation/pending-deferred-tool";
import type { PendingElicitation } from "@/features/conversation/pending-elicitation";
import { adaptPermissionToDeferredTool } from "@/features/conversation/permission-as-deferred-tool";
import type { AgentModelSection } from "@/lib/api";
import { createWinthorpeQueryClient } from "@/lib/query-client";
import { WorkspaceComposer } from "./index";

// ── Mock fixtures ─────────────────────────────────────────────────────
// Shapes copied from the real streaming payloads (see tests in
// `composer/index.test.tsx` for the upstream reference).

const MODEL_SECTIONS: AgentModelSection[] = [
	{
		id: "claude",
		label: "Claude",
		options: [
			{
				id: "sonnet-4-5",
				provider: "claude",
				label: "Sonnet 4.5",
				cliModel: "claude-sonnet-4-5",
				effortLevels: ["low", "medium", "high", "max"],
				supportsFastMode: true,
			},
		],
	},
];

const MOCK_PERMISSION: PendingPermission = {
	permissionId: "perm-mock-1",
	toolName: "Bash",
	toolInput: {
		command: "rm -f /Users/kevin/nix-config/.git/hooks/post-checkout",
	},
	title: null,
	description: null,
};

// State A now routes through the same `pendingDeferredTool` channel as State B
// so the two states share one UI (GenericDeferredToolPanel). The adapter
// wraps the permission request in a PendingDeferredTool shape with a
// reserved toolUseId prefix.
const MOCK_PERMISSION_AS_DEFERRED_TOOL =
	adaptPermissionToDeferredTool(MOCK_PERMISSION);

const MOCK_DEFERRED_TOOL: PendingDeferredTool = {
	provider: "claude",
	modelId: "sonnet-4-5",
	resolvedModel: "claude-sonnet-4-5",
	providerSessionId: null,
	workingDirectory: "/tmp/winthorpe",
	permissionMode: null,
	toolUseId: "tool-mock-1",
	toolName: "Bash",
	toolInput: {
		command: "rm -f /Users/kevin/nix-config/.git/hooks/post-checkout",
		description: "Remove the post-checkout git hook",
	},
};

const MOCK_FORM_ELICITATION: PendingElicitation = {
	provider: "claude",
	modelId: "sonnet-4-5",
	resolvedModel: "claude-sonnet-4-5",
	providerSessionId: null,
	workingDirectory: "/tmp/winthorpe",
	elicitationId: "elicit-mock-form",
	serverName: "vercel",
	message: "Configure the new deployment target.",
	mode: "form",
	url: null,
	requestedSchema: {
		type: "object",
		properties: {
			projectName: {
				type: "string",
				title: "Project name",
				description: "Human-friendly name shown in the dashboard.",
				minLength: 1,
				maxLength: 64,
			},
			port: {
				type: "integer",
				title: "Port",
				description: "Port the dev server should bind to.",
				minimum: 1,
				maximum: 65535,
				default: 3000,
			},
			environment: {
				type: "string",
				title: "Environment",
				description: "Which environment to deploy into.",
				enum: ["development", "staging", "production"],
				enumNames: ["Development", "Staging", "Production"],
			},
			autoDeploy: {
				type: "boolean",
				title: "Auto-deploy on push",
				description: "Automatically redeploy when main changes.",
				default: false,
			},
		},
		required: ["projectName", "environment"],
	},
};

const MOCK_URL_ELICITATION: PendingElicitation = {
	provider: "claude",
	modelId: "sonnet-4-5",
	resolvedModel: "claude-sonnet-4-5",
	providerSessionId: null,
	workingDirectory: "/tmp/winthorpe",
	elicitationId: "elicit-mock-url",
	serverName: "auth-server",
	message: "Finish signing in in the browser to continue.",
	mode: "url",
	url: "https://example.com/oauth/authorize?client_id=winthorpe&state=abc",
	requestedSchema: null,
};

const MOCK_ASK_USER_QUESTION: PendingDeferredTool = {
	provider: "claude",
	modelId: "sonnet-4-5",
	resolvedModel: "claude-sonnet-4-5",
	providerSessionId: null,
	workingDirectory: "/tmp/winthorpe",
	permissionMode: null,
	toolUseId: "tool-ask-1",
	toolName: "AskUserQuestion",
	toolInput: {
		questions: [
			{
				header: "UI",
				question: "Which UI path should we take?",
				options: [
					{
						label: "Patch existing",
						description: "Keep the current layout and patch the flow.",
					},
					{
						label: "Build new",
						description: "Create a dedicated approval surface.",
					},
				],
			},
			{
				header: "Checks",
				question: "Which checks should run before merge?",
				multiSelect: true,
				options: [
					{ label: "Vitest", description: "Run the frontend test suite." },
					{ label: "Typecheck", description: "Run the repository typecheck." },
				],
			},
		],
	},
};

// ── Shared composer prop defaults ─────────────────────────────────────
// The real app wires these through `WorkspaceComposerContainer`; here we
// provide the minimal set so `<WorkspaceComposer>` renders faithfully.

type ComposerProps = React.ComponentProps<typeof WorkspaceComposer>;

function baseComposerProps(contextKey: string): ComposerProps {
	return {
		contextKey,
		onSubmit: () => {},
		onStop: () => {},
		sending: false,
		selectedModelId: "sonnet-4-5",
		modelSections: MODEL_SECTIONS,
		modelsLoading: false,
		onSelectModel: () => {},
		provider: "claude",
		effortLevel: "high",
		onSelectEffort: () => {},
		permissionMode: "acceptEdits",
		onChangePermissionMode: () => {},
		fastMode: false,
		onChangeFastMode: () => {},
		restoreDraft: null,
		restoreImages: [],
		restoreFiles: [],
		restoreCustomTags: [],
		restoreNonce: 0,
		slashCommands: [],
		workspaceRootPath: "/tmp/winthorpe",
		pendingElicitation: null,
		pendingDeferredTool: null,
		hasPlanReview: false,
	};
}

// ── Story harness ─────────────────────────────────────────────────────
// Wraps previews in the same providers the real App.tsx uses so queries
// and Radix tooltips resolve without errors.

let sharedQueryClient: QueryClient | null = null;
function getQueryClient(): QueryClient {
	if (sharedQueryClient == null) {
		sharedQueryClient = createWinthorpeQueryClient();
	}
	return sharedQueryClient;
}

function Harness({
	children,
	label,
	width = 720,
}: {
	children: ReactNode;
	label: string;
	width?: number;
}) {
	return (
		<div
			className="flex flex-col gap-2"
			style={{ width: `${width}px`, maxWidth: "100%" }}
		>
			<div className="text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
				{label}
			</div>
			{/*
			 * `mt-auto px-4 pb-4 pt-0` + the wrapping `<div>` copy the layout
			 * from `conversation/index.tsx` so the permission bar's `-mb-px`
			 * overlap with the composer is reproduced exactly.
			 */}
			<div className="mt-auto px-4 pb-4 pt-0">
				<div>{children}</div>
			</div>
		</div>
	);
}

function withProviders(story: () => ReactNode) {
	return (
		<QueryClientProvider client={getQueryClient()}>
			<TooltipProvider>{story()}</TooltipProvider>
		</QueryClientProvider>
	);
}

// ── Meta ──────────────────────────────────────────────────────────────

const meta: Meta = {
	title: "Features/Composer/Interaction States",
	parameters: {
		layout: "fullscreen",
	},
	tags: ["autodocs"],
};

export default meta;

type Story = StoryObj;

// ── Stories ───────────────────────────────────────────────────────────
// Each story renders the *real* WorkspaceComposer — the only difference
// between states is which mock fixture (if any) is supplied as a prop.

export const Default: Story = {
	render: () =>
		withProviders(() => (
			<Harness label="Default — Lexical editor + toolbar">
				<WorkspaceComposer {...baseComposerProps("story:default")} />
			</Harness>
		)),
};

export const StateAPermissionBar: Story = {
	name: "A — Permission request",
	render: () =>
		withProviders(() => (
			<Harness label="A — Permission request (shared panel with B)">
				<WorkspaceComposer
					{...baseComposerProps("story:permission")}
					pendingDeferredTool={MOCK_PERMISSION_AS_DEFERRED_TOOL}
				/>
			</Harness>
		)),
};

export const StateBDeferredTool: Story = {
	name: "B — Deferred tool (generic)",
	render: () =>
		withProviders(() => (
			<Harness label="B — Generic deferred tool panel">
				<WorkspaceComposer
					{...baseComposerProps("story:deferred-tool")}
					pendingDeferredTool={MOCK_DEFERRED_TOOL}
				/>
			</Harness>
		)),
};

export const StateBAskUserQuestion: Story = {
	name: "B — Ask user question",
	render: () =>
		withProviders(() => (
			<Harness label="B — AskUserQuestion deferred tool panel">
				<WorkspaceComposer
					{...baseComposerProps("story:ask-user-question")}
					pendingDeferredTool={MOCK_ASK_USER_QUESTION}
				/>
			</Harness>
		)),
};

export const StateCFormElicitation: Story = {
	name: "C — Form elicitation",
	render: () =>
		withProviders(() => (
			<Harness label="C — Form elicitation panel">
				<WorkspaceComposer
					{...baseComposerProps("story:form-elicitation")}
					pendingElicitation={MOCK_FORM_ELICITATION}
				/>
			</Harness>
		)),
};

export const StateCUrlElicitation: Story = {
	name: "C — URL elicitation",
	render: () =>
		withProviders(() => (
			<Harness label="C — URL elicitation panel">
				<WorkspaceComposer
					{...baseComposerProps("story:url-elicitation")}
					pendingElicitation={MOCK_URL_ELICITATION}
				/>
			</Harness>
		)),
};

export const AllStates: Story = {
	name: "All states (grid)",
	render: () =>
		withProviders(() => (
			// `maxHeight: 100dvh` + `overflow-y-auto` lets the grid scroll when the
			// combined height of the six harnesses exceeds the Storybook canvas
			// (which is fixed to the viewport because the meta uses
			// `layout: "fullscreen"`). Without this the overflowing harnesses at
			// the bottom get clipped.
			<div
				className="flex flex-wrap items-start gap-8 overflow-y-auto p-6"
				style={{ maxHeight: "100dvh" }}
			>
				<Harness label="Default" width={560}>
					<WorkspaceComposer {...baseComposerProps("story:grid-default")} />
				</Harness>
				<Harness label="A — Permission request" width={560}>
					<WorkspaceComposer
						{...baseComposerProps("story:grid-permission")}
						pendingDeferredTool={MOCK_PERMISSION_AS_DEFERRED_TOOL}
					/>
				</Harness>
				<Harness label="B — Deferred tool" width={560}>
					<WorkspaceComposer
						{...baseComposerProps("story:grid-deferred-tool")}
						pendingDeferredTool={MOCK_DEFERRED_TOOL}
					/>
				</Harness>
				<Harness label="B — Ask user question" width={560}>
					<WorkspaceComposer
						{...baseComposerProps("story:grid-ask-user-question")}
						pendingDeferredTool={MOCK_ASK_USER_QUESTION}
					/>
				</Harness>
				<Harness label="C — Form elicitation" width={560}>
					<WorkspaceComposer
						{...baseComposerProps("story:grid-form-elicitation")}
						pendingElicitation={MOCK_FORM_ELICITATION}
					/>
				</Harness>
				<Harness label="C — URL elicitation" width={560}>
					<WorkspaceComposer
						{...baseComposerProps("story:grid-url-elicitation")}
						pendingElicitation={MOCK_URL_ELICITATION}
					/>
				</Harness>
			</div>
		)),
};
