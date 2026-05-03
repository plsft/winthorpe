import { defineConfig, devices } from "@playwright/test";

// Winthorpe runs against WebKit because that's the production webview on
// macOS Tauri. On Windows (the project's primary target) Tauri uses
// WebView2 / Chromium — so WebKit also acts as the stricter engine that
// surfaces issues Chromium would miss. We don't run a separate chromium
// project here because two engines doubles CI time without catching
// distinct bugs in practice.

const PORT = 1430;

// Webkit on a Linux CI runner has a noticeably slower cold start than
// chromium — the default 5 s expect timeout can fire before the React
// shell finishes its first render. Bumping these makes the harness
// resilient to runner load without papering over real regressions:
// genuinely broken tests still time out (just at the higher bound).
const CI = !!process.env.CI;
const EXPECT_TIMEOUT_MS = CI ? 20_000 : 5_000;
const ACTION_TIMEOUT_MS = CI ? 15_000 : 5_000;
const NAVIGATION_TIMEOUT_MS = CI ? 30_000 : 15_000;

export default defineConfig({
	testDir: "./e2e/tests",
	fullyParallel: true,
	forbidOnly: CI,
	retries: CI ? 2 : 0,
	workers: CI ? 1 : undefined,
	reporter: CI ? [["list"], ["html", { open: "never" }]] : "list",
	expect: {
		timeout: EXPECT_TIMEOUT_MS,
	},
	use: {
		baseURL: `http://localhost:${PORT}`,
		trace: "retain-on-failure",
		video: "retain-on-failure",
		actionTimeout: ACTION_TIMEOUT_MS,
		navigationTimeout: NAVIGATION_TIMEOUT_MS,
	},
	projects: [
		{
			name: "webkit",
			use: { ...devices["Desktop Safari"] },
		},
	],
	webServer: {
		command: "bun x vite --config vite.e2e.config.ts",
		url: `http://localhost:${PORT}`,
		reuseExistingServer: !CI,
		timeout: 120_000,
		stdout: "pipe",
		stderr: "pipe",
	},
});
