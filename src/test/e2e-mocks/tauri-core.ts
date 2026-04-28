// Browser stand-in for `@tauri-apps/api/core`. Vite alias redirects every
// import of the real module here when `VITE_WINTHORPE_E2E=1`.

import { runInvoke } from "./invoke-defaults";

export async function invoke<T = unknown>(
	command: string,
	args?: unknown,
): Promise<T> {
	return (await runInvoke(command, args)) as T;
}

export class Channel<T = unknown> {
	onmessage: ((event: T) => void) | null = null;

	toJSON(): string {
		return "__WINTHORPE_E2E_CHANNEL__";
	}
}

export function convertFileSrc(path: string): string {
	return path;
}
