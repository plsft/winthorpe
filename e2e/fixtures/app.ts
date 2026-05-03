import { test as base, type Page } from "@playwright/test";

// Shared Playwright fixture that boots the Winthorpe shell past its first-run
// gates so specs start from the main workspace surface:
//   - stubs `get_app_settings` to mark onboarding completed so the overlay
//     never appears
//   - exposes `window.__WINTHORPE_E2E__` as a hook for specs to override
//     individual invoke commands before the app boots

declare global {
	interface Window {
		__WINTHORPE_E2E__?: {
			invokeOverrides?: Record<string, (args?: unknown) => unknown>;
		};
	}
}

type WinthorpeFixtures = {
	app: Page;
};

export const test = base.extend<WinthorpeFixtures>({
	app: async ({ page }, use) => {
		await page.addInitScript(() => {
			window.__WINTHORPE_E2E__ = {
				invokeOverrides: {
					get_app_settings: () => ({ "app.onboarding_completed": "true" }),
				},
			};
		});

		await page.goto("/");
		// Wait for the React shell to render its top-level chrome before
		// handing control to the spec. Without this, webkit's cold start on
		// a Linux CI runner can lose the race against a spec's first
		// `toBeVisible` assertion. Anchoring on the workspace sidebar is
		// safe because every onboarding-completed boot lands there.
		await page
			.getByRole("complementary", { name: "Workspace sidebar" })
			.waitFor({ state: "visible" });
		await use(page);
	},
});

export { expect } from "@playwright/test";
