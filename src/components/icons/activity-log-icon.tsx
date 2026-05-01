import type { SVGProps } from "react";

/**
 * Activity log icon — three stacked log lines with a leading ▸ marker on
 * the top line to suggest "live tail." Custom SVG so the affordance is
 * unmistakable next to the gear (Settings) icon.
 */
export function ActivityLogIcon(props: SVGProps<SVGSVGElement>) {
	return (
		<svg
			viewBox="0 0 16 16"
			fill="none"
			stroke="currentColor"
			strokeWidth="1.4"
			strokeLinecap="round"
			strokeLinejoin="round"
			aria-hidden="true"
			{...props}
		>
			{/* Leading caret on the active row — suggests live tail */}
			<path d="M2.5 4l1.4 1-1.4 1" />
			{/* Three log lines, top is the live one */}
			<line x1="6" y1="5" x2="13.5" y2="5" />
			<line x1="3" y1="9" x2="13.5" y2="9" />
			<line x1="3" y1="13" x2="10.5" y2="13" />
		</svg>
	);
}
