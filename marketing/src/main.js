import Alpine from "alpinejs";
import "./styles/app.css";
import { initEasterEgg } from "./easter-egg.js";

window.Alpine = Alpine;

// Active-nav helper. The header partial calls `nav.is(href)` to decide which
// link to highlight — done here rather than per-page so we add a route once.
Alpine.data("nav", () => ({
	is(href) {
		const current = window.location.pathname.replace(/\/$/, "") || "/";
		const target = href.replace(/\/$/, "") || "/";
		return current === target;
	},
}));

// Mobile menu state. Header reuses this on every page.
Alpine.data("siteHeader", () => ({
	open: false,
	close() {
		this.open = false;
	},
}));

Alpine.start();

initEasterEgg();
