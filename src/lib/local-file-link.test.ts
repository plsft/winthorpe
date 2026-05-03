import { describe, expect, it } from "vitest";
import { parseLocalFileLink } from "./local-file-link";

describe("parseLocalFileLink", () => {
	it("parses absolute file links with GitHub-style line fragments", () => {
		expect(
			parseLocalFileLink(
				"/Users/liangeqiang/project/src/App.tsx#L161C4",
				"/Users/liangeqiang/project",
			),
		).toEqual({
			path: "/Users/liangeqiang/project/src/App.tsx",
			line: 161,
			column: 4,
		});
	});

	it("parses absolute file links with trailing line and column", () => {
		expect(
			parseLocalFileLink(
				"/Users/liangeqiang/project/src/App.tsx:42:7",
				"/Users/liangeqiang/project",
			),
		).toEqual({
			path: "/Users/liangeqiang/project/src/App.tsx",
			line: 42,
			column: 7,
		});
	});

	it("resolves workspace-relative file links", () => {
		expect(
			parseLocalFileLink("src-tauri/src/lib.rs#L12", "/tmp/winthorpe"),
		).toEqual({
			path: "/tmp/winthorpe/src-tauri/src/lib.rs",
			line: 12,
		});
	});

	it("ignores external urls", () => {
		expect(
			parseLocalFileLink(
				"https://example.com/src/App.tsx#L10",
				"/tmp/winthorpe",
			),
		).toBeNull();
	});

	it("requires a workspace root for relative paths", () => {
		expect(parseLocalFileLink("src/App.tsx#L10")).toBeNull();
	});
});
