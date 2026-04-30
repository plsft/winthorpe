import "monaco-editor/min/vs/editor/editor.main.css";
import type * as Monaco from "monaco-editor";
import editorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import cssWorker from "monaco-editor/esm/vs/language/css/css.worker?worker";
import htmlWorker from "monaco-editor/esm/vs/language/html/html.worker?worker";
import jsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";
import tsWorker from "monaco-editor/esm/vs/language/typescript/ts.worker?worker";

type MonacoModule = typeof Monaco;
type StandaloneEditor = Monaco.editor.IStandaloneCodeEditor;
type StandaloneDiffEditor = Monaco.editor.IStandaloneDiffEditor;

type MonacoRuntime = {
	monaco: MonacoModule;
};

type DisposableLike = {
	dispose(): void;
};

type FileEditorController = {
	editor: StandaloneEditor;
	dispose(): void;
	getValue(): string;
	setValue(value: string): void;
	revealPosition(line?: number, column?: number): void;
	onDidChangeModelContent(callback: (value: string) => void): DisposableLike;
	/** Swap the active model. Returns false if no cached model and no content provided. */
	switchFile(
		path: string,
		content?: string,
		line?: number,
		column?: number,
	): boolean;
};

type DiffEditorController = {
	editor: StandaloneDiffEditor;
	dispose(): void;
	setTexts(options: {
		originalText: string;
		modifiedText: string;
		inline: boolean;
	}): void;
};

let runtimePromise: Promise<MonacoRuntime> | null = null;

/** Content cache for pre-fetched files — avoids IPC on first switch. */
const fileContentCache = new Map<string, string>();

type EditorTheme = "light" | "dark";

/** Pending theme applied once runtime is ready (or the current one). */
let desiredTheme: EditorTheme = detectInitialTheme();

function detectInitialTheme(): EditorTheme {
	if (typeof document === "undefined") {
		return "dark";
	}
	return document.documentElement.classList.contains("dark") ? "dark" : "light";
}

function themeId(theme: EditorTheme): string {
	return theme === "dark" ? "winthorpe-editor-dark" : "winthorpe-editor-light";
}

export async function createFileEditor(options: {
	container: HTMLElement;
	path: string;
	content: string;
	line?: number;
	column?: number;
}): Promise<FileEditorController> {
	const runtime = await ensureRuntime();
	const { monaco } = runtime;

	const language = resolveLanguageId(monaco, options.path);

	// Single model shared across all file switches — avoids editor.setModel()
	// which causes a blank frame during the detach→attach cycle.
	const model = monaco.editor.createModel(options.content, language);

	// Seed content cache for future switches
	fileContentCache.set(options.path, options.content);

	const editor = monaco.editor.create(options.container, {
		automaticLayout: true,
		bracketPairColorization: { enabled: true },
		fontFamily:
			'"SF Mono","Monaco","Cascadia Mono","Roboto Mono","Menlo",monospace',
		fontLigatures: true,
		fontSize: 13,
		lineHeight: 21,
		minimap: { enabled: false },
		model,
		padding: { top: 14, bottom: 24 },
		renderValidationDecorations: "editable",
		scrollBeyondLastLine: false,
		smoothScrolling: true,
		tabSize: 2,
		theme: themeId(desiredTheme),
		wordWrap: "on",
	});

	revealEditorPosition(editor, options.line, options.column);

	const currentModel = model;

	return {
		editor,
		dispose() {
			editor.dispose();
		},
		getValue() {
			return currentModel.getValue();
		},
		setValue(value: string) {
			if (currentModel.getValue() === value) {
				return;
			}

			currentModel.setValue(value);
		},
		revealPosition(line?: number, column?: number) {
			revealEditorPosition(editor, line, column);
		},
		onDidChangeModelContent(callback) {
			return currentModel.onDidChangeContent(() => {
				callback(currentModel.getValue());
			});
		},
		switchFile(path: string, content?: string, line?: number, column?: number) {
			// Resolve content: explicit param → cache → give up
			const resolvedContent = content ?? fileContentCache.get(path);
			if (resolvedContent === undefined) {
				return false;
			}

			// In-place update: setValue + setModelLanguage on the SAME model.
			// Unlike editor.setModel(), this never detaches the DOM → zero blank frames.
			currentModel.setValue(resolvedContent);

			const nextLanguage = resolveLanguageId(monaco, path);
			if (nextLanguage && currentModel.getLanguageId() !== nextLanguage) {
				monaco.editor.setModelLanguage(currentModel, nextLanguage);
			}

			// Keep cache fresh for future switches back to this file
			fileContentCache.set(path, resolvedContent);

			revealEditorPosition(editor, line, column);
			return true;
		},
	};
}

export async function createDiffEditor(options: {
	container: HTMLElement;
	path: string;
	originalText: string;
	modifiedText: string;
	inline: boolean;
}): Promise<DiffEditorController> {
	const runtime = await ensureRuntime();
	const { monaco } = runtime;
	const language = resolveLanguageId(monaco, options.path);

	const originalUri = monaco.Uri.file(options.path).with({
		query: "winthorpe-review=original",
	});
	const modifiedUri = monaco.Uri.file(options.path).with({
		query: "winthorpe-review=modified",
	});
	monaco.editor.getModel(originalUri)?.dispose();
	monaco.editor.getModel(modifiedUri)?.dispose();

	const originalModel = monaco.editor.createModel(
		options.originalText,
		language,
		originalUri,
	);
	const modifiedModel = monaco.editor.createModel(
		options.modifiedText,
		language,
		modifiedUri,
	);

	const editor = monaco.editor.createDiffEditor(options.container, {
		automaticLayout: true,
		enableSplitViewResizing: true,
		fontFamily:
			'"SF Mono","Monaco","Cascadia Mono","Roboto Mono","Menlo",monospace',
		fontLigatures: true,
		fontSize: 13,
		hideUnchangedRegions: {
			enabled: true,
			contextLineCount: 4,
			minimumLineCount: 2,
			revealLineCount: 3,
		},
		lineHeight: 21,
		minimap: { enabled: false },
		originalEditable: false,
		padding: { top: 14, bottom: 24 },
		readOnly: true,
		renderOverviewRuler: false,
		renderSideBySide: !options.inline,
		scrollBeyondLastLine: false,
		smoothScrolling: true,
		theme: themeId(desiredTheme),
	});

	editor.setModel({
		original: originalModel,
		modified: modifiedModel,
	});

	return {
		editor,
		dispose() {
			editor.dispose();
			originalModel.dispose();
			modifiedModel.dispose();
		},
		setTexts({ originalText, modifiedText, inline }) {
			if (originalModel.getValue() !== originalText) {
				originalModel.setValue(originalText);
			}
			if (modifiedModel.getValue() !== modifiedText) {
				modifiedModel.setValue(modifiedText);
			}
			editor.updateOptions({ renderSideBySide: !inline });
		},
	};
}

/** Cache file contents so future switchFile calls resolve instantly (no IPC). */
export function preWarmFileContents(
	files: ReadonlyArray<{ absolutePath: string; content: string }>,
) {
	for (const file of files) {
		fileContentCache.set(file.absolutePath, file.content);
	}
}

export function syncVirtualFile(path: string, content: string) {
	fileContentCache.set(path, content);
}

async function ensureRuntime(): Promise<MonacoRuntime> {
	if (!runtimePromise) {
		runtimePromise = (async () => {
			const monaco = await import("monaco-editor");

			installMonacoEnvironment();
			installEditorTheme(monaco);
			installThemeObserver(monaco);

			return { monaco };
		})();
	}

	return runtimePromise;
}

// Sync Monaco's theme with the app's `dark` class on <html>. Avoids having
// callers import this module just to push a theme update, which would pull
// Monaco's runtime into the critical path on every theme change.
function installThemeObserver(monaco: MonacoModule) {
	if (
		typeof document === "undefined" ||
		typeof MutationObserver === "undefined"
	) {
		return;
	}
	const syncTheme = () => {
		const nextTheme = detectInitialTheme();
		if (nextTheme === desiredTheme) {
			return;
		}
		desiredTheme = nextTheme;
		monaco.editor.setTheme(themeId(nextTheme));
	};
	const observer = new MutationObserver(syncTheme);
	observer.observe(document.documentElement, {
		attributes: true,
		attributeFilter: ["class"],
	});
	syncTheme();
}

function installMonacoEnvironment() {
	const target = globalThis as typeof globalThis & {
		MonacoEnvironment?: {
			getWorker: (_moduleId: string, label: string) => Worker;
		};
	};

	if (target.MonacoEnvironment) {
		return;
	}

	target.MonacoEnvironment = {
		getWorker(_moduleId, label) {
			switch (label) {
				case "json":
					return new jsonWorker();
				case "css":
				case "scss":
				case "less":
					return new cssWorker();
				case "html":
				case "handlebars":
				case "razor":
					return new htmlWorker();
				case "typescript":
				case "javascript":
					return new tsWorker();
				default:
					return new editorWorker();
			}
		},
	};
}

function installEditorTheme(monaco: MonacoModule) {
	// Tokyo Night palette (Folke Lemaitre's port). Chosen as the default dark
	// theme because it's the most-requested community theme for Monaco / VS
	// Code and has good contrast on the LCD displays most Winthorpe users
	// run. Token-color mapping below tracks the original Tokyo Night spec
	// (storm variant): bg #1a1b26, fg #c0caf5, blue #7aa2f7, purple #bb9af7.
	monaco.editor.defineTheme("winthorpe-editor-dark", {
		base: "vs-dark",
		inherit: true,
		rules: [
			{ token: "", foreground: "c0caf5" },
			{ token: "comment", foreground: "565f89", fontStyle: "italic" },
			{ token: "string", foreground: "9ece6a" },
			{ token: "string.escape", foreground: "2ac3de" },
			{ token: "keyword", foreground: "bb9af7" },
			{ token: "keyword.flow", foreground: "bb9af7" },
			{ token: "keyword.json", foreground: "bb9af7" },
			{ token: "number", foreground: "ff9e64" },
			{ token: "regexp", foreground: "b4f9f8" },
			{ token: "operator", foreground: "89ddff" },
			{ token: "delimiter", foreground: "89ddff" },
			{ token: "type", foreground: "2ac3de" },
			{ token: "type.identifier", foreground: "2ac3de" },
			{ token: "identifier", foreground: "c0caf5" },
			{ token: "variable", foreground: "c0caf5" },
			{ token: "variable.predefined", foreground: "f7768e" },
			{ token: "variable.parameter", foreground: "e0af68" },
			{ token: "function", foreground: "7aa2f7" },
			{ token: "tag", foreground: "f7768e" },
			{ token: "attribute.name", foreground: "bb9af7" },
			{ token: "attribute.value", foreground: "9ece6a" },
			{ token: "metatag", foreground: "f7768e" },
			{ token: "constant", foreground: "ff9e64" },
			{ token: "constant.language", foreground: "bb9af7" },
		],
		colors: {
			"editor.background": "#1a1b26",
			"editor.foreground": "#c0caf5",
			"editor.lineHighlightBackground": "#24283b",
			"editor.lineHighlightBorder": "#00000000",
			"editor.selectionBackground": "#33467c",
			"editor.inactiveSelectionBackground": "#2a3057",
			"editor.wordHighlightBackground": "#33467c66",
			"editor.wordHighlightStrongBackground": "#33467c99",
			"editor.findMatchBackground": "#3d59a1aa",
			"editor.findMatchHighlightBackground": "#3d59a155",
			"editorCursor.foreground": "#c0caf5",
			"editorWhitespace.foreground": "#3b4261",
			"editorIndentGuide.background1": "#292e42",
			"editorIndentGuide.activeBackground1": "#3b4261",
			"editorLineNumber.foreground": "#3b4261",
			"editorLineNumber.activeForeground": "#737aa2",
			"editorGutter.background": "#1a1b26",
			"editorWidget.background": "#16161e",
			"editorWidget.border": "#1b1e2e",
			"editorSuggestWidget.background": "#16161e",
			"editorSuggestWidget.border": "#1b1e2e",
			"editorSuggestWidget.selectedBackground": "#283457",
			"editorHoverWidget.background": "#16161e",
			"editorHoverWidget.border": "#1b1e2e",
			"editorBracketMatch.background": "#3d59a155",
			"editorBracketMatch.border": "#3d59a1",
			"scrollbarSlider.background": "#9aa5ce20",
			"scrollbarSlider.hoverBackground": "#9aa5ce40",
			"scrollbarSlider.activeBackground": "#9aa5ce55",
			"minimap.background": "#1a1b26",
			"diffEditor.insertedLineBackground": "#9ece6a18",
			"diffEditor.insertedTextBackground": "#9ece6a33",
			"diffEditor.removedLineBackground": "#f7768e18",
			"diffEditor.removedTextBackground": "#f7768e33",
			"diffEditorGutter.insertedLineBackground": "#9ece6a26",
			"diffEditorGutter.removedLineBackground": "#f7768e26",
			"diffEditorOverview.insertedForeground": "#9ece6a99",
			"diffEditorOverview.removedForeground": "#f7768e99",
			"diffEditor.diagonalFill": "#c0caf508",
		},
	});
	monaco.editor.defineTheme("winthorpe-editor-light", {
		base: "vs",
		inherit: true,
		rules: [
			{ token: "comment", foreground: "7a7775" },
			{ token: "string", foreground: "8a6b3d" },
			{ token: "keyword", foreground: "8a3d51" },
			{ token: "number", foreground: "8a6e2f" },
			{ token: "regexp", foreground: "5a6b3d" },
			{ token: "type.identifier", foreground: "3d4d75" },
			{ token: "identifier", foreground: "1a1918" },
			{ token: "delimiter", foreground: "5a5857" },
		],
		colors: {
			"editor.background": "#FFFFFF",
			"editor.foreground": "#1a1918",
			"editor.lineHighlightBackground": "#f4f3f1",
			"editor.lineHighlightBorder": "#00000000",
			"editor.selectionBackground": "#c9d9ef",
			"editor.inactiveSelectionBackground": "#dde3ec",
			"editor.wordHighlightBackground": "#c9d9ef88",
			"editor.wordHighlightStrongBackground": "#a8c1e288",
			"editorCursor.foreground": "#1a1918",
			"editorWhitespace.foreground": "#c7c5c2",
			"editorIndentGuide.background1": "#eceae6",
			"editorIndentGuide.activeBackground1": "#c7c5c2",
			"editorLineNumber.foreground": "#a4a19d",
			"editorLineNumber.activeForeground": "#1a1918",
			"editorGutter.background": "#FFFFFF",
			"editorWidget.background": "#f8f7f5",
			"editorWidget.border": "#e4e2de",
			"editorSuggestWidget.background": "#f8f7f5",
			"editorSuggestWidget.border": "#e4e2de",
			"editorHoverWidget.background": "#f8f7f5",
			"editorHoverWidget.border": "#e4e2de",
			"scrollbarSlider.background": "#1a191826",
			"scrollbarSlider.hoverBackground": "#1a191840",
			"scrollbarSlider.activeBackground": "#1a191855",
			"minimap.background": "#FFFFFF",
			"diffEditor.insertedLineBackground": "#2ea04318",
			"diffEditor.insertedTextBackground": "#2ea04333",
			"diffEditor.removedLineBackground": "#da363318",
			"diffEditor.removedTextBackground": "#da363333",
			"diffEditorGutter.insertedLineBackground": "#2ea04326",
			"diffEditorGutter.removedLineBackground": "#da363326",
			"diffEditorOverview.insertedForeground": "#2ea04399",
			"diffEditorOverview.removedForeground": "#da363399",
			"diffEditor.diagonalFill": "#1a19180a",
		},
	});
	monaco.editor.setTheme(themeId(desiredTheme));
}

function resolveLanguageId(
	monaco: MonacoModule,
	path: string,
): string | undefined {
	const normalizedPath = path.replace(/\\/g, "/");
	const fileName = normalizedPath.split("/").pop()?.toLowerCase() ?? "";
	const extension = fileName.includes(".")
		? fileName.slice(fileName.lastIndexOf("."))
		: "";

	const explicitMap: Record<string, string> = {
		".cjs": "javascript",
		".css": "css",
		".go": "go",
		".html": "html",
		".java": "java",
		".js": "javascript",
		".json": "json",
		".jsx": "javascript",
		".md": "markdown",
		".mjs": "javascript",
		".py": "python",
		".rs": "rust",
		".scss": "scss",
		".sh": "shell",
		".sql": "sql",
		".toml": "ini",
		".ts": "typescript",
		".tsx": "typescript",
		".txt": "plaintext",
		".yaml": "yaml",
		".yml": "yaml",
	};

	if (fileName === "dockerfile") {
		return "dockerfile";
	}

	if (fileName.endsWith(".test.tsx") || fileName.endsWith(".spec.tsx")) {
		return "typescript";
	}

	if (explicitMap[extension]) {
		return explicitMap[extension];
	}

	return monaco.languages.getLanguages().find((language) => {
		const extensions = language.extensions ?? [];
		const filenames = language.filenames ?? [];
		return extensions.includes(extension) || filenames.includes(fileName);
	})?.id;
}

function revealEditorPosition(
	editor: StandaloneEditor,
	line?: number,
	column?: number,
) {
	if (!line) {
		return;
	}

	const position = {
		lineNumber: Math.max(1, line),
		column: Math.max(1, column ?? 1),
	};
	editor.setPosition(position);
	editor.revealPositionInCenter(position);
	editor.focus();
}
