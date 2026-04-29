import { useId } from "react";
import { resolveTheme, useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";

// Stylized W logo built from parallelogram tiles with a cascade-flip
// animation. Pure SVG + CSS keyframes so this component has no runtime
// dependency on lottie-web (which a separate component,
// fast-mode-lottie-icon.tsx, still uses for its own purposes).
//
// Visual: four diagonal "bars" in the W silhouette, each split into two
// stacked parallelogram tiles. Tiles flip horizontally (scaleX 1 → 0 → 1)
// in a cascading order that traces the W writing path:
//
//      bar 0       bar 3
//        \           /
//         \   bar 1 /
//          \ /   \ /
//          bar 2  V
//
// The animation runs in two cascade waves with a hold between, matching
// the original H Lottie's pacing (compressed for snappier loop).

interface WinthorpeLogoAnimatedProps {
	/** CSS width/height — accepts numbers (px) or any CSS length. */
	size?: string | number;
	/** Loop the animation. true by default — set false for a one-shot reveal. */
	loop?: boolean;
	/** Autoplay on mount. true by default. */
	autoplay?: boolean;
	className?: string;
}

// Geometry of one diagonal bar split into two parallelogram tiles.
// Coordinates are in a 100x100 viewBox; the W spans the full viewBox.
//
// Each bar is described by:
//   x1,y1 → top of bar; x2,y2 → bottom of bar; w → bar width
//
// Bars 0/3 are the outer downstrokes; bars 1/2 are the inner upstrokes.
// Together they trace W when read left→right top→bottom.
const BAR_GEOMETRY = [
	// Outer left downstroke
	{ x1: 14, y1: 14, x2: 32, y2: 86, w: 10, cascade: 0 },
	// Inner left upstroke (rises from bottom-center to top of inner peak)
	{ x1: 32, y1: 86, x2: 50, y2: 42, w: 10, cascade: 1 },
	// Inner right downstroke (falls from inner peak to bottom-center)
	{ x1: 50, y1: 42, x2: 68, y2: 86, w: 10, cascade: 2 },
	// Outer right upstroke
	{ x1: 68, y1: 86, x2: 86, y2: 14, w: 10, cascade: 3 },
] as const;

/**
 * Build parallelogram tile paths for one bar segment.
 *
 * Each bar splits into 3 stacked tiles separated by a thin gap. Three
 * tiles per bar matches the H logo's vertical-column granularity (3 tiles
 * per upright on the original) and gives the cascade animation enough
 * frames to feel like a real flip-cascade rather than a one-shot flicker.
 */
function barTilePaths(bar: (typeof BAR_GEOMETRY)[number]): string[] {
	const { x1, y1, x2, y2, w } = bar;
	const dx = x2 - x1;
	const dy = y2 - y1;
	const len = Math.sqrt(dx * dx + dy * dy);
	// Unit vectors: along the bar (ux, uy) and perpendicular (px, py).
	const ux = dx / len;
	const uy = dy / len;
	const px = (-dy / len) * (w / 2);
	const py = (dx / len) * (w / 2);

	const TILES_PER_BAR = 3;
	const GAP = 1.5; // inter-tile gap in viewBox units
	const tileLen = (len - GAP * (TILES_PER_BAR - 1)) / TILES_PER_BAR;
	const stride = tileLen + GAP;

	const out: string[] = [];
	for (let i = 0; i < TILES_PER_BAR; i++) {
		const startT = i * stride;
		const endT = startT + tileLen;
		const sx = x1 + ux * startT;
		const sy = y1 + uy * startT;
		const ex = x1 + ux * endT;
		const ey = y1 + uy * endT;
		out.push(
			`M ${sx - px} ${sy - py} L ${sx + px} ${sy + py} L ${ex + px} ${ey + py} L ${ex - px} ${ey - py} Z`,
		);
	}
	return out;
}

export function WinthorpeLogoAnimated({
	size,
	loop = true,
	autoplay = true,
	className,
}: WinthorpeLogoAnimatedProps) {
	const { settings } = useSettings();
	const fill = resolveTheme(settings.theme) === "light" ? "#0E0E0E" : "#FAFAFA";
	// useId gives each instance unique animation names so multiple
	// WinthorpeLogoAnimated on the same page don't collide.
	const id = useId().replace(/:/g, "-");

	// Cascade waves. Each bar contributes 3 tiles (top → bottom). The
	// animation runs in 3 waves of 4 tiles each (one per bar): wave 1 flips
	// all bars' top tile, wave 2 their middle, wave 3 their bottom. Inside
	// a wave, the 4 bars fire 0.10s apart so the eye reads the cascade as
	// "writing the W".
	const tiles: Array<{ d: string; delay: number }> = [];
	const WAVE_HOLD = 0.6; // seconds between successive waves
	const PER_BAR_DELAY = 0.1;
	for (const bar of BAR_GEOMETRY) {
		const paths = barTilePaths(bar);
		paths.forEach((d, tileIndex) => {
			tiles.push({
				d,
				delay: tileIndex * WAVE_HOLD + bar.cascade * PER_BAR_DELAY,
			});
		});
	}

	// Total: (3 waves * 0.6s) + (0.4s last wave settle) + 0.4s loop pad.
	const duration = "2.6s";

	return (
		<>
			<style>{`
				@keyframes wt-tile-flip-${id} {
					0%, 8%   { transform: scaleX(1); }
					12%, 20% { transform: scaleX(0); }
					24%, 100% { transform: scaleX(1); }
				}
			`}</style>
			<svg
				viewBox="0 0 100 100"
				className={className}
				width={size}
				height={size}
				aria-hidden="true"
				style={{ display: "block", overflow: "visible" }}
			>
				{tiles.map((tile, i) => (
					<path
						key={i}
						d={tile.d}
						fill={fill}
						style={{
							transformOrigin: "center",
							transformBox: "fill-box",
							animationName: autoplay ? `wt-tile-flip-${id}` : "none",
							animationDuration: duration,
							animationDelay: `${tile.delay}s`,
							animationIterationCount: loop ? "infinite" : 1,
							animationTimingFunction: "cubic-bezier(0.4, 0, 0.2, 1)",
							animationFillMode: "both",
						}}
					/>
				))}
			</svg>
		</>
	);
}

/**
 * Static (non-animated) variant of the W logo. Used by callers that want
 * the logo shape without the animation cost (Storybook poster frames,
 * splash mockups, marketing thumbnails).
 */
export function WinthorpeLogoStatic({
	size,
	className,
}: {
	size?: string | number;
	className?: string;
}) {
	const { settings } = useSettings();
	const fill = resolveTheme(settings.theme) === "light" ? "#0E0E0E" : "#FAFAFA";
	const tiles: string[] = [];
	for (const bar of BAR_GEOMETRY) {
		tiles.push(...barTilePaths(bar));
	}
	return (
		<svg
			viewBox="0 0 100 100"
			className={cn(className)}
			width={size}
			height={size}
			aria-hidden="true"
			style={{ display: "block" }}
		>
			{tiles.map((d, i) => (
				<path key={i} d={d} fill={fill} />
			))}
		</svg>
	);
}
