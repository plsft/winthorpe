export function getCurrentWindow() {
	return {
		onCloseRequested: async () => () => {},
		close: async () => {},
		setTitle: async () => {},
		show: async () => {},
		hide: async () => {},
		setBadgeCount: async () => {},
		// window-title-bar polls these on every mount; without them the
		// component throws `win.isMaximized is not a function`, the React
		// shell never finishes rendering, and every E2E spec fails its
		// first `toBeVisible`.
		isMaximized: async () => false,
		onResized: async () => () => {},
	};
}
