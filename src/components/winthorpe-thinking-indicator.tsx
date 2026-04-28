import { WinthorpeLogoAnimated } from "@/components/winthorpe-logo-animated";
import { cn } from "@/lib/utils";

type WinthorpeThinkingIndicatorProps = {
	size?: number | string;
	className?: string;
};

export function WinthorpeThinkingIndicator({
	size = 14,
	className,
}: WinthorpeThinkingIndicatorProps) {
	return (
		<span
			aria-hidden="true"
			data-slot="winthorpe-thinking-indicator"
			className={cn(
				"inline-flex shrink-0 items-center justify-center",
				className,
			)}
			style={{ width: size, height: size }}
		>
			<WinthorpeLogoAnimated size={size} className="shrink-0 opacity-80" />
		</span>
	);
}
