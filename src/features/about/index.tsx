import {
	getVersion as getAppVersion,
	getTauriVersion,
} from "@tauri-apps/api/app";
import {
	arch as osArch,
	platform as osPlatform,
	version as osVersion,
} from "@tauri-apps/plugin-os";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { WinthorpeLogoAnimated } from "@/components/winthorpe-logo-animated";
import { errorMessage } from "@/lib/utils";

type Versions = {
	winthorpe: string;
	tauri: string;
	osPlatform: string;
	osVersion: string;
	osArch: string;
	commit?: string;
};

/**
 * About dialog — the closest thing Winthorpe has to a "splash with build
 * info." Shown from `Help → About Winthorpe`. Keep it visually close to
 * the splash screen: same animated W logo, same dark surface, just with
 * a one-glance version table underneath.
 */
export function AboutDialog({
	open,
	onClose,
}: {
	open: boolean;
	onClose: () => void;
}) {
	const [versions, setVersions] = useState<Versions | null>(null);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		if (!open) return;
		let cancelled = false;

		void Promise.all([
			getAppVersion(),
			getTauriVersion(),
			osPlatform(),
			osVersion(),
			osArch(),
		])
			.then(([appVer, tauriVer, plat, ver, archStr]) => {
				if (cancelled) return;
				setVersions({
					winthorpe: appVer,
					tauri: tauriVer,
					osPlatform: prettyPlatform(plat),
					osVersion: ver,
					osArch: archStr,
					// Build commit injected by Vite at build time (define block
					// in vite.config.ts) — falls through if not configured.
					commit:
						typeof __WINTHORPE_BUILD_COMMIT__ === "string"
							? __WINTHORPE_BUILD_COMMIT__
							: undefined,
				});
			})
			.catch((e) => {
				if (!cancelled) setError(errorMessage(e));
			});

		return () => {
			cancelled = true;
		};
	}, [open]);

	return (
		<Dialog
			open={open}
			onOpenChange={(next) => {
				if (!next) onClose();
			}}
		>
			<DialogContent className="gap-0 overflow-hidden p-0 sm:max-w-md">
				<DialogHeader className="sr-only">
					<DialogTitle>About Winthorpe</DialogTitle>
				</DialogHeader>

				{/* Splash band — animated W on a soft gradient backdrop. Mirrors
				    the splash screen so the feel is consistent. */}
				<div className="flex flex-col items-center gap-3 bg-gradient-to-b from-card via-background to-background px-6 pt-8 pb-5">
					<WinthorpeLogoAnimated size={56} />
					<div className="text-center">
						<div className="text-base font-semibold tracking-tight text-foreground">
							Winthorpe
						</div>
						<div className="text-[11px] text-muted-foreground">
							The local-first IDE for orchestrating coding agents.
						</div>
					</div>
				</div>

				{error ? (
					<div className="border-t border-destructive/40 bg-destructive/10 px-6 py-3 text-xs text-destructive">
						Couldn't read system info: {error}
					</div>
				) : null}

				<dl className="grid grid-cols-[120px_1fr] gap-x-3 gap-y-1.5 border-t border-border/60 px-6 py-5 text-[12px]">
					<Row label="Winthorpe" value={versions?.winthorpe} />
					{versions?.commit ? (
						<Row label="Build" value={versions.commit.slice(0, 12)} mono />
					) : null}
					<Row label="Tauri runtime" value={versions?.tauri} />
					<Row
						label="Operating system"
						value={
							versions
								? `${versions.osPlatform} ${versions.osVersion}`
								: undefined
						}
					/>
					<Row label="Architecture" value={versions?.osArch} />
				</dl>

				<div className="border-t border-border/60 bg-muted/30 px-6 py-3 text-[11px] text-muted-foreground">
					Apache 2.0 · A Windows-first fork of{" "}
					<a
						href="https://github.com/dohooo/helmor"
						className="underline decoration-dotted underline-offset-2 hover:text-foreground"
						target="_blank"
						rel="noreferrer"
					>
						Helmor
					</a>
					. Plural Software.
				</div>

				<div className="flex items-center justify-between border-t border-border/60 bg-muted/30 px-4 py-2.5">
					<a
						href="https://github.com/plsft/winthorpe"
						target="_blank"
						rel="noreferrer"
						className="inline-flex items-center gap-1.5 text-[12px] text-foreground/80 hover:text-foreground"
					>
						<svg
							viewBox="0 0 16 16"
							className="size-3.5"
							fill="currentColor"
							aria-hidden="true"
						>
							<path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.012 8.012 0 0 0 16 8c0-4.42-3.58-8-8-8z" />
						</svg>
						github.com/plsft/winthorpe
					</a>
					<Button size="sm" variant="ghost" onClick={onClose}>
						Close
					</Button>
				</div>
			</DialogContent>
		</Dialog>
	);
}

function Row({
	label,
	value,
	mono,
}: {
	label: string;
	value: string | undefined;
	mono?: boolean;
}) {
	return (
		<>
			<dt className="text-muted-foreground">{label}</dt>
			<dd className={mono ? "font-mono text-foreground" : "text-foreground"}>
				{value ?? "…"}
			</dd>
		</>
	);
}

function prettyPlatform(p: string): string {
	if (p === "windows") return "Windows";
	if (p === "macos") return "macOS";
	if (p === "linux") return "Linux";
	return p;
}

declare const __WINTHORPE_BUILD_COMMIT__: string | undefined;
