#!/usr/bin/env node
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
/**
 * Render src/assets/render-banner.html to src/assets/readme-banner.png
 * via headless Chromium. The HTML draws the W-tile pattern + animated
 * reveal that matches the in-app brand. We capture a single static frame
 * (not the animation) sized for a GitHub README — wide aspect ratio.
 *
 * Run with:
 *   bun scripts/render-readme-banner.mjs
 */
import { chromium } from "playwright";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, "..");
const htmlPath = resolve(repoRoot, "src/assets/render-banner.html");
const outPath = resolve(repoRoot, "src/assets/readme-banner.png");

// GitHub renders README images at ~2x DPR on retina. Render at 1320x500
// for a clean wide aspect that crops cleanly when GitHub auto-scales.
const VIEWPORT = { width: 1320, height: 500 };

async function main() {
	const browser = await chromium.launch({ channel: "chromium" });
	try {
		const context = await browser.newContext({
			viewport: VIEWPORT,
			deviceScaleFactor: 2, // retina-quality output
		});
		const page = await context.newPage();
		await page.goto(`file:///${htmlPath.replaceAll("\\", "/")}`);
		// Wait for the JS-driven SVG layout to settle. The animation runs on
		// CSS keyframes; we capture the natural starting frame, which has
		// the W tiles fully drawn in white-on-near-black.
		await page.waitForTimeout(400);
		await page.screenshot({
			path: outPath,
			type: "png",
			fullPage: false,
			omitBackground: false,
		});
		console.log(`Wrote ${outPath}`);
	} finally {
		await browser.close();
	}
}

main().catch((err) => {
	console.error(err);
	process.exit(1);
});
