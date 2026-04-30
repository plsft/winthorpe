/**
 * Trading Places easter egg.
 *
 * The product is named after Louis Winthorpe III. Type the Konami code or
 * the word "winthorpe" anywhere on the site to surface a quote modal.
 *
 * Event flow: this module fires `winthorpe:easter-egg` on `window` with
 * the chosen quote in `event.detail`. The footer modal (Alpine) listens
 * via `@winthorpe:easter-egg.window`. Decoupled this way so the easter
 * egg works no matter which page the slot lives on.
 */

const QUOTES = [
	{ line: "Looking good, Billy Ray!", attr: "Louis Winthorpe III" },
	{ line: "Feeling good, Louis!", attr: "Billy Ray Valentine" },
	{ line: "Pork bellies! I knew it!", attr: "Mortimer Duke" },
	{
		line: "When I'm finished with him, he'll have nothing — no job, no money, nothing.",
		attr: "Randolph Duke",
	},
];

// Konami sequence. Letters are stored lowercase and compared case-insensitively
// so caps-lock / shift don't break it.
const KONAMI = [
	"arrowup",
	"arrowup",
	"arrowdown",
	"arrowdown",
	"arrowleft",
	"arrowright",
	"arrowleft",
	"arrowright",
	"b",
	"a",
];

const TRIGGER_WORD = "winthorpe";

function pickQuote() {
	return QUOTES[Math.floor(Math.random() * QUOTES.length)];
}

function fire() {
	window.dispatchEvent(
		new CustomEvent("winthorpe:easter-egg", { detail: pickQuote() }),
	);
}

export function initEasterEgg() {
	let konamiIndex = 0;
	let typedBuffer = "";

	window.addEventListener("keydown", (e) => {
		// Don't fire while the user is typing in a form field.
		const t = e.target;
		if (
			t instanceof HTMLInputElement ||
			t instanceof HTMLTextAreaElement ||
			(t instanceof HTMLElement && t.isContentEditable)
		) {
			return;
		}

		const key = e.key.toLowerCase();

		// Konami arm. We compare lowercased so shift/caps-lock don't break "b"/"a".
		if (key === KONAMI[konamiIndex]) {
			konamiIndex += 1;
			if (konamiIndex === KONAMI.length) {
				fire();
				konamiIndex = 0;
			}
		} else if (key === KONAMI[0]) {
			// The wrong key happened to be the start of the sequence — keep
			// the streak alive at index 1 instead of resetting to 0.
			konamiIndex = 1;
		} else {
			konamiIndex = 0;
		}

		// Typed-word arm. Append printable single-char keys to a rolling buffer.
		if (key.length === 1) {
			typedBuffer = (typedBuffer + key).slice(-TRIGGER_WORD.length);
			if (typedBuffer === TRIGGER_WORD) {
				fire();
				typedBuffer = "";
			}
		}
	});
}
