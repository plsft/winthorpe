import { ClaudeIcon, OpenAIIcon } from "@/components/icons";
import type { AgentLoginStatusResult } from "@/lib/api";
import type { AgentLoginItem } from "./types";

export function buildAgentLoginItems(
	status?: AgentLoginStatusResult | null,
): AgentLoginItem[] {
	return [
		{
			icon: ClaudeIcon,
			provider: "claude",
			label: "Claude Code",
			description: status?.claude
				? "Signed in and ready to run in local workspaces."
				: "Sign in to Claude Code to use Anthropic models in Winthorpe.",
			status: status?.claude ? "ready" : "needsSetup",
		},
		{
			icon: OpenAIIcon,
			provider: "codex",
			label: "Codex",
			description: status?.codex
				? "Signed in and ready to run OpenAI models in Winthorpe."
				: "Sign in to Codex to use OpenAI models in Winthorpe.",
			status: status?.codex ? "ready" : "needsSetup",
		},
	];
}
