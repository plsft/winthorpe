import { defineConfig, devices } from "@playwright/test";

// Winthorpe's production target is Tauri's macOS WebKit webview, so the E2E
// harness runs against WebKit only — Chromium/Firefox add test time without
// getting us closer to production fidelity.

const PORT = 1430;

export default defineConfig({
	testDir: "./e2e/tests",
	fullyParallel: true,
	forbidOnly: !!process.env.CI,
	retries: process.env.CI ? 2 : 0,
	workers: process.env.CI ? 1 : undefined,
	reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
	use: {
		baseURL: `http://localhost:${PORT}`,
		trace: "retain-on-failure",
		video: "retain-on-failure",
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
		reuseExistingServer: !process.env.CI,
		timeout: 120_000,
		stdout: "pipe",
		stderr: "pipe",
	},
});
