import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { WinthorpeLogoStatic } from "@/components/winthorpe-logo-animated";
import { isMac } from "@/lib/platform";
import { cn } from "@/lib/utils";

/**
 * Custom window title bar.
 *
 * Replaces the OS-native chrome (`tauri.conf.json` sets `decorations: false`)
 * with our own bar so we can render the Winthorpe brand, integrate Mica
 * tinting, and keep the same UI conventions on Windows + macOS.
 *
 * Drag-region pattern (workjet-derived): the **central spacer** carries the
 * `data-tauri-drag-region` attribute, NOT the parent. WebView2 on Windows
 * intercepts pointer events on descendants of a drag region even when those
 * descendants declare `data-tauri-drag-region="false"` — so the safe pattern
 * is to scope drag to a single inert spacer between the logo and the controls.
 */

interface WindowTitleBarProps {
	className?: string;
}

export function WindowTitleBar({ className }: WindowTitleBarProps) {
	const mac = isMac();
	const [maximized, setMaximized] = useState(false);

	useEffect(() => {
		const win = getCurrentWindow();
		let dispose: (() => void) | null = null;
		win
			.isMaximized()
			.then(setMaximized)
			.catch(() => {});
		win
			.onResized(async () => {
				try {
					setMaximized(await win.isMaximized());
				} catch {
					/* swallow — window may be tearing down */
				}
			})
			.then((un) => {
				dispose = un;
			})
			.catch(() => {});
		return () => {
			dispose?.();
		};
	}, []);

	const onMinimize = useCallback(async () => {
		try {
			await getCurrentWindow().minimize();
		} catch (e) {
			console.error("[title-bar] minimize failed:", e);
		}
	}, []);

	const onToggleMaximize = useCallback(async () => {
		try {
			await getCurrentWindow().toggleMaximize();
		} catch (e) {
			console.error("[title-bar] toggleMaximize failed:", e);
		}
	}, []);

	const onClose = useCallback(async () => {
		try {
			await getCurrentWindow().close();
		} catch (e) {
			console.error("[title-bar] close failed:", e);
		}
	}, []);

	return (
		<header
			data-slot="window-title-bar"
			data-platform={mac ? "macos" : "windows"}
			className={cn(
				"relative z-50 flex h-9 shrink-0 select-none items-center justify-between border-b border-border/40 bg-background/60 backdrop-blur-sm",
				// On macOS, leave room on the left for the (hidden) traffic
				// lights area so the logo doesn't overlap them when the user
				// re-enables system decorations for accessibility.
				mac && "pl-20",
				className,
			)}
		>
			{/* Left: brand + product name */}
			<div className="flex h-full items-center gap-2 px-3">
				<WinthorpeLogoStatic
					size={16}
					className="shrink-0 opacity-90 [&_path]:fill-foreground"
				/>
				<span className="text-[12px] font-medium tracking-tight text-foreground/80">
					Winthorpe
				</span>
			</div>

			{/* Center: drag region. Empty by design — dragging only happens
			    here, never on the parent (see component header doc). */}
			<div
				data-tauri-drag-region
				className="h-full flex-1 cursor-default"
				aria-hidden="true"
			/>

			{/* Right: window controls. On macOS the system normally renders
			    these on the LEFT, but since we rely on `decorations: false`
			    on every platform for visual consistency, we keep them on the
			    right everywhere. macOS users get a familiar UX from any
			    cross-platform Electron app (VS Code, Slack, Discord all do this). */}
			<div className="flex h-full items-center">
				<TitleBarButton
					label="Minimize"
					onClick={onMinimize}
					className="hover:bg-foreground/10"
				>
					<Minus className="size-3.5" strokeWidth={1.5} />
				</TitleBarButton>
				<TitleBarButton
					label={maximized ? "Restore" : "Maximize"}
					onClick={onToggleMaximize}
					className="hover:bg-foreground/10"
				>
					{maximized ? (
						<RestoreIcon />
					) : (
						<Square className="size-3" strokeWidth={1.5} />
					)}
				</TitleBarButton>
				<TitleBarButton
					label="Close"
					onClick={onClose}
					// Windows red close button convention.
					className="hover:bg-[#e81123] hover:text-white"
				>
					<X className="size-3.5" strokeWidth={1.6} />
				</TitleBarButton>
			</div>
		</header>
	);
}

interface TitleBarButtonProps {
	label: string;
	onClick: () => void;
	className?: string;
	children: React.ReactNode;
}

function TitleBarButton({
	label,
	onClick,
	className,
	children,
}: TitleBarButtonProps) {
	return (
		<button
			type="button"
			aria-label={label}
			title={label}
			onClick={onClick}
			className={cn(
				"flex h-full w-11 items-center justify-center text-foreground/70 transition-colors cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring/50",
				className,
			)}
		>
			{children}
		</button>
	);
}

/**
 * Restore icon — two overlapping squares, the back one offset up-right.
 * Matches the Windows 11 explorer chrome convention.
 */
function RestoreIcon() {
	return (
		<svg
			width="12"
			height="12"
			viewBox="0 0 12 12"
			fill="none"
			stroke="currentColor"
			strokeWidth="1.2"
			aria-hidden="true"
		>
			<rect x="2.5" y="3.5" width="6.5" height="6.5" />
			<path d="M4.5 3.5 V2 H10 V7.5 H8.5" />
		</svg>
	);
}
