import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { requestQuit } from "@/lib/api";

export function QuitConfirmDialog({
	sendingSessionIds,
}: {
	sendingSessionIds: Set<string>;
}) {
	const [open, setOpen] = useState(false);
	const sendingRef = useRef(sendingSessionIds);
	sendingRef.current = sendingSessionIds;

	const handleQuit = useCallback(async (force: boolean) => {
		setOpen(false);
		await requestQuit(force);
	}, []);

	useEffect(() => {
		let disposed = false;
		let unlisten: (() => void) | undefined;

		// Rust intercepts every OS-level exit path (close button, Cmd+Q,
		// app-menu Quit, programmatic ExitRequested) and emits this
		// event. We're the only gate that knows about in-flight tasks.
		void listen("winthorpe://quit-requested", () => {
			if (sendingRef.current.size === 0) {
				void requestQuit(false);
				return;
			}
			setOpen(true);
		}).then((fn) => {
			if (disposed) {
				fn();
				return;
			}
			unlisten = fn;
		});

		return () => {
			disposed = true;
			unlisten?.();
		};
	}, []);

	const count = sendingSessionIds.size;

	return (
		<ConfirmDialog
			open={open}
			onOpenChange={setOpen}
			title="Quit Winthorpe?"
			description={
				count === 1
					? "There is 1 task in progress. Quitting now will cancel it."
					: `There are ${count} tasks in progress. Quitting now will cancel them.`
			}
			confirmLabel="Quit anyway"
			onConfirm={() => void handleQuit(true)}
		/>
	);
}
