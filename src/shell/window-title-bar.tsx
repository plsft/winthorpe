import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuSeparator,
	DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
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
	onOpenAbout?: () => void;
	onOpenActivityLog?: () => void;
	onOpenCostDashboard?: () => void;
	onOpenSettings?: () => void;
	onNewWorkspace?: () => void;
	onAddRepository?: () => void;
	onToggleTheme?: () => void;
	onToggleLeftSidebar?: () => void;
	onToggleRightSidebar?: () => void;
}

/**
 * Dispatch a global keyboard event so focused widgets (Monaco, the
 * composer, terminal panels) handle "Save" / "Find" via their existing
 * keybindings without us threading a callback to every editor instance.
 */
function dispatchKeyShortcut(
	key: string,
	modifiers: { ctrl?: boolean; shift?: boolean } = {},
) {
	const target = document.activeElement ?? document.body;
	target.dispatchEvent(
		new KeyboardEvent("keydown", {
			key,
			ctrlKey: modifiers.ctrl ?? false,
			shiftKey: modifiers.shift ?? false,
			bubbles: true,
			cancelable: true,
		}),
	);
}

type MenuId = "file" | "edit" | "view" | "help";

export function WindowTitleBar({
	className,
	onOpenAbout,
	onOpenActivityLog,
	onOpenCostDashboard,
	onOpenSettings,
	onNewWorkspace,
	onAddRepository,
	onToggleTheme,
	onToggleLeftSidebar,
	onToggleRightSidebar,
}: WindowTitleBarProps) {
	const mac = isMac();
	const [maximized, setMaximized] = useState(false);
	const [openMenu, setOpenMenu] = useState<MenuId | null>(null);

	// Alt+F / Alt+E / Alt+V / Alt+H — Windows menu mnemonic convention.
	// Listen at the window level so the shortcut works no matter what's
	// focused. We preventDefault on the matching event so the browser's
	// native Alt-handling (which steals focus on some Chromium versions)
	// stays out of the way. Esc closes any open menu.
	useEffect(() => {
		const handler = (event: KeyboardEvent) => {
			if (event.key === "Escape" && openMenu) {
				setOpenMenu(null);
				return;
			}
			if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
				return;
			}
			// `event.key` for Alt+F is "f" on Windows/Linux; on macOS Alt is
			// the Option key and produces a different glyph (e.g. "ƒ"), but
			// we still match against the underlying letter via `event.code`.
			const codeChar =
				event.code.startsWith("Key") && event.code.length === 4
					? event.code.charAt(3).toLowerCase()
					: null;
			const letter = codeChar ?? event.key.toLowerCase();
			const next: Record<string, MenuId> = {
				f: "file",
				e: "edit",
				v: "view",
				h: "help",
			};
			const target = next[letter];
			if (!target) return;
			event.preventDefault();
			setOpenMenu(target);
		};
		window.addEventListener("keydown", handler);
		return () => window.removeEventListener("keydown", handler);
	}, [openMenu]);

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
			{/* Left: brand + product name + menu strip */}
			<div className="flex h-full items-center gap-1 pl-3 pr-1">
				<WinthorpeLogoStatic
					size={16}
					className="shrink-0 opacity-90 [&_path]:fill-foreground"
				/>
				<span className="mr-2 text-[12px] font-medium tracking-tight text-foreground/80">
					Winthorpe
				</span>

				<MenuButton
					label="File"
					mnemonic="F"
					open={openMenu === "file"}
					onOpenChange={(o) => setOpenMenu(o ? "file" : null)}
				>
					<DropdownMenuItem onSelect={() => onNewWorkspace?.()}>
						New Workspace
						<MenuShortcut>Ctrl+N</MenuShortcut>
					</DropdownMenuItem>
					<DropdownMenuItem onSelect={() => onAddRepository?.()}>
						Add Repository…
					</DropdownMenuItem>
					<DropdownMenuSeparator />
					<DropdownMenuItem
						onSelect={() => dispatchKeyShortcut("s", { ctrl: true })}
					>
						Save
						<MenuShortcut>Ctrl+S</MenuShortcut>
					</DropdownMenuItem>
					<DropdownMenuItem
						onSelect={() => dispatchKeyShortcut("w", { ctrl: true })}
					>
						Close Tab
						<MenuShortcut>Ctrl+W</MenuShortcut>
					</DropdownMenuItem>
					<DropdownMenuSeparator />
					<DropdownMenuItem
						onSelect={() => {
							void getCurrentWindow().close();
						}}
					>
						Quit
						<MenuShortcut>Alt+F4</MenuShortcut>
					</DropdownMenuItem>
				</MenuButton>

				<MenuButton
					label="Edit"
					mnemonic="E"
					open={openMenu === "edit"}
					onOpenChange={(o) => setOpenMenu(o ? "edit" : null)}
				>
					<DropdownMenuItem
						onSelect={() => dispatchKeyShortcut("z", { ctrl: true })}
					>
						Undo
						<MenuShortcut>Ctrl+Z</MenuShortcut>
					</DropdownMenuItem>
					<DropdownMenuItem
						onSelect={() =>
							dispatchKeyShortcut("z", { ctrl: true, shift: true })
						}
					>
						Redo
						<MenuShortcut>Ctrl+Shift+Z</MenuShortcut>
					</DropdownMenuItem>
					<DropdownMenuSeparator />
					<DropdownMenuItem
						onSelect={() => dispatchKeyShortcut("x", { ctrl: true })}
					>
						Cut
						<MenuShortcut>Ctrl+X</MenuShortcut>
					</DropdownMenuItem>
					<DropdownMenuItem
						onSelect={() => dispatchKeyShortcut("c", { ctrl: true })}
					>
						Copy
						<MenuShortcut>Ctrl+C</MenuShortcut>
					</DropdownMenuItem>
					<DropdownMenuItem
						onSelect={() => dispatchKeyShortcut("v", { ctrl: true })}
					>
						Paste
						<MenuShortcut>Ctrl+V</MenuShortcut>
					</DropdownMenuItem>
					<DropdownMenuSeparator />
					<DropdownMenuItem
						onSelect={() => dispatchKeyShortcut("f", { ctrl: true })}
					>
						Find
						<MenuShortcut>Ctrl+F</MenuShortcut>
					</DropdownMenuItem>
				</MenuButton>

				<MenuButton
					label="View"
					mnemonic="V"
					open={openMenu === "view"}
					onOpenChange={(o) => setOpenMenu(o ? "view" : null)}
				>
					<DropdownMenuItem onSelect={() => onToggleLeftSidebar?.()}>
						Toggle Workspaces
						<MenuShortcut>Ctrl+B</MenuShortcut>
					</DropdownMenuItem>
					<DropdownMenuItem onSelect={() => onToggleRightSidebar?.()}>
						Toggle Inspector
						<MenuShortcut>Ctrl+J</MenuShortcut>
					</DropdownMenuItem>
					<DropdownMenuSeparator />
					<DropdownMenuItem onSelect={() => onToggleTheme?.()}>
						Toggle Theme
					</DropdownMenuItem>
					<DropdownMenuSeparator />
					<DropdownMenuItem onSelect={() => onOpenActivityLog?.()}>
						Activity Log
					</DropdownMenuItem>
					<DropdownMenuItem onSelect={() => onOpenCostDashboard?.()}>
						Cost &amp; Tokens
					</DropdownMenuItem>
					<DropdownMenuItem onSelect={() => onOpenSettings?.()}>
						Settings…
					</DropdownMenuItem>
				</MenuButton>

				<MenuButton
					label="Help"
					mnemonic="H"
					open={openMenu === "help"}
					onOpenChange={(o) => setOpenMenu(o ? "help" : null)}
				>
					<DropdownMenuItem
						onSelect={() => {
							void import("@tauri-apps/plugin-opener").then(({ openUrl }) =>
								openUrl("https://github.com/plsft/winthorpe"),
							);
						}}
					>
						Documentation
					</DropdownMenuItem>
					<DropdownMenuItem
						onSelect={() => {
							void import("@tauri-apps/plugin-opener").then(({ openUrl }) =>
								openUrl("https://github.com/plsft/winthorpe/issues/new"),
							);
						}}
					>
						Report an Issue
					</DropdownMenuItem>
					<DropdownMenuItem
						onSelect={() => {
							void import("@tauri-apps/plugin-opener").then(({ openUrl }) =>
								openUrl("https://github.com/plsft/winthorpe/releases"),
							);
						}}
					>
						Releases
					</DropdownMenuItem>
					<DropdownMenuSeparator />
					<DropdownMenuItem onSelect={() => onOpenAbout?.()}>
						About Winthorpe
					</DropdownMenuItem>
				</MenuButton>
			</div>

			{/* Center: drag region. Drag only fires on this inert spacer — see
			    component header doc for the WebView2 caveat. */}
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
 * Top-of-window menu trigger. Controlled so the parent can open it via
 * Alt+letter (Windows mnemonic convention). The mnemonic letter inside
 * the label is rendered with a 1px underline so users can see which Alt
 * shortcut maps to which menu — the same UX every native Windows app uses.
 */
function MenuButton({
	label,
	mnemonic,
	open,
	onOpenChange,
	children,
}: {
	label: string;
	mnemonic: string;
	open: boolean;
	onOpenChange: (open: boolean) => void;
	children: React.ReactNode;
}) {
	return (
		<DropdownMenu open={open} onOpenChange={onOpenChange}>
			<DropdownMenuTrigger asChild>
				<button
					type="button"
					className="rounded-md px-2 py-0.5 text-[12px] font-medium text-foreground/80 transition-colors hover:bg-foreground/10 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring/50 data-[state=open]:bg-foreground/10 data-[state=open]:text-foreground"
				>
					{renderLabelWithMnemonic(label, mnemonic)}
				</button>
			</DropdownMenuTrigger>
			<DropdownMenuContent
				align="start"
				sideOffset={2}
				className="min-w-[200px]"
			>
				{children}
			</DropdownMenuContent>
		</DropdownMenu>
	);
}

/**
 * Underlines the first matching `mnemonic` letter inside `label` so the
 * Alt+letter affordance is visible (Windows native convention). Falls
 * back to plain text if the mnemonic isn't present in the label.
 */
function renderLabelWithMnemonic(label: string, mnemonic: string) {
	const idx = label.toLowerCase().indexOf(mnemonic.toLowerCase());
	if (idx < 0) return label;
	return (
		<>
			{label.slice(0, idx)}
			<span className="underline decoration-foreground/60 underline-offset-2">
				{label.charAt(idx)}
			</span>
			{label.slice(idx + 1)}
		</>
	);
}

/** Right-aligned keyboard hint inside a DropdownMenuItem. */
function MenuShortcut({ children }: { children: React.ReactNode }) {
	return (
		<span className="ml-auto pl-4 text-[11px] tabular-nums text-muted-foreground">
			{children}
		</span>
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
