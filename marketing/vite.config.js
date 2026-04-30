import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { resolve } from "node:path";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

const root = resolve(import.meta.dirname);
const partialsDir = resolve(root, "src/partials");

/**
 * Tiny include processor for Vite's index-html pipeline.
 *
 * Use either:
 *   <% header %>            → loads `src/partials/header.html`
 *   <% footer %>            → loads `src/partials/footer.html`
 *   <% any-partial-name %>  → loads `src/partials/<name>.html`
 *
 * Tokens that don't match a partial file are passed through unchanged so
 * normal HTML never gets accidentally consumed.
 */
function partialsPlugin() {
	return {
		name: "winthorpe-partials",
		transformIndexHtml: {
			order: "pre",
			handler(html) {
				return html.replace(/<%\s*([\w./-]+)\s*%>/g, (match, name) => {
					const file = name.endsWith(".html") ? name : `${name}.html`;
					const full = resolve(partialsDir, file);
					if (existsSync(full)) {
						return readFileSync(full, "utf-8");
					}
					return match;
				});
			},
		},
	};
}

/**
 * Walk the marketing root and collect every `index.html` we find. Vite's
 * MPA mode needs each page listed explicitly under `rollupOptions.input`,
 * but writing them out by hand goes stale every time we add a page.
 */
function collectPages(dir, out = {}) {
	for (const entry of readdirSync(dir)) {
		if (entry === "node_modules" || entry === "dist" || entry.startsWith(".")) {
			continue;
		}
		const full = resolve(dir, entry);
		const stat = statSync(full);
		if (stat.isDirectory()) {
			collectPages(full, out);
		} else if (entry === "index.html") {
			const rel = full.slice(root.length + 1).replace(/\\/g, "/");
			const name =
				rel === "index.html" ? "main" : rel.replace(/\/index\.html$/, "");
			out[name] = full;
		}
	}
	return out;
}

export default defineConfig({
	plugins: [partialsPlugin(), tailwindcss()],
	build: {
		rollupOptions: {
			input: collectPages(root),
		},
	},
	server: {
		port: 5173,
	},
});
