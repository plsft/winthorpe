// Stage claude-code + codex + bun + gh + glab into `sidecar/dist/vendor/`
// for Tauri to ship as bundle resources. Cross-platform: macOS arm64/x64,
// Windows x64. Linux can be added later by extending detectTarget().

import { execFileSync, execSync } from "node:child_process";
import {
	chmodSync,
	cpSync,
	existsSync,
	mkdirSync,
	readdirSync,
	realpathSync,
	rmSync,
	statSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SIDECAR_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const NODE_MODULES = join(SIDECAR_ROOT, "node_modules");
const DIST_VENDOR = join(SIDECAR_ROOT, "dist", "vendor");
const BUNDLE_CACHE = join(SIDECAR_ROOT, ".bundle-cache");

// Bumping: update version + sha256, wipe sidecar/.bundle-cache. Checksums:
//   gh:   https://github.com/cli/cli/releases/download/v$VER/gh_${VER}_checksums.txt
//   glab: https://gitlab.com/gitlab-org/cli/-/releases/v$VER/downloads/checksums.txt
//
// Windows checksums must be filled in on first build. Run with the env var
// WINTHORPE_VENDOR_SKIP_SHA_CHECK=1 to bootstrap; the script will print the
// real SHA256 of each downloaded artifact, which you then paste back into
// this file and re-run without the bypass.
const GH_VERSION = "2.91.0";
const GH_SHA256 = {
	mac_arm64: "20446cd714d9fa1b69fbd410deade3731f38fe09a2b980c8488aa388dd320ada",
	mac_amd64: "8806784f93603fe6d3f95c3583a08df38f175df9ebc123dc8b15f919329980e2",
	win_amd64: "ced3e6f4bb5a9865056b594b7ad0cf42137dc92c494346f1ca705b5dbf14c88e",
} as const;

const GLAB_VERSION = "1.93.0";
const GLAB_SHA256 = {
	mac_arm64: "6d6ffa97d430b5e7ff912e64dbac14703acc57967df654be1950ae71858d5b6f",
	mac_amd64: "79d1a4f933919689c5fb7774feb1dd08f30b9c896dff4283b4a7387689ee0531",
	win_amd64: "e07ea21f9a3df8eac5e1c16136c186154769504355a44195b47c44e410a39097",
} as const;

const SKIP_SHA_CHECK = process.env.WINTHORPE_VENDOR_SKIP_SHA_CHECK === "1";

// ---------------------------------------------------------------------------
// Platform detection
// ---------------------------------------------------------------------------

type NodeArch = "arm64" | "x64";
type Platform = "darwin" | "win32";

interface TargetInfo {
	platform: Platform;
	/** `@anthropic-ai/claude-code` vendor subdir naming (`<arch>-<os>`). */
	ccVendorArch: string;
	/** `@openai/codex-<os>-<arch>` is the npm optional-dep package. */
	codexPkg: string;
	/** Target triple used as the subdir inside the codex platform package. */
	codexTriple: string;
	/** `gh` and `glab` Windows zips use `amd64`/`arm64` for arch in the filename. */
	ghArch: "arm64" | "amd64";
	glabArch: "arm64" | "amd64";
	/** Filename suffix added to vendored binaries (`.exe` on Windows). */
	exeSuffix: string;
	/** SHA256 keys for gh/glab. */
	ghShaKey: keyof typeof GH_SHA256;
	glabShaKey: keyof typeof GLAB_SHA256;
}

function detectTarget(): TargetInfo {
	const arch = process.arch as NodeArch;

	if (process.platform === "darwin") {
		switch (arch) {
			case "arm64":
				return {
					platform: "darwin",
					ccVendorArch: "arm64-darwin",
					codexPkg: "@openai/codex-darwin-arm64",
					codexTriple: "aarch64-apple-darwin",
					ghArch: "arm64",
					glabArch: "arm64",
					exeSuffix: "",
					ghShaKey: "mac_arm64",
					glabShaKey: "mac_arm64",
				};
			case "x64":
				return {
					platform: "darwin",
					ccVendorArch: "x64-darwin",
					codexPkg: "@openai/codex-darwin-x64",
					codexTriple: "x86_64-apple-darwin",
					ghArch: "amd64",
					glabArch: "amd64",
					exeSuffix: "",
					ghShaKey: "mac_amd64",
					glabShaKey: "mac_amd64",
				};
		}
	}

	if (process.platform === "win32") {
		if (arch !== "x64") {
			throw new Error(
				`[stage-vendor] Windows on ${arch} is not yet supported (only x64 is).`,
			);
		}
		return {
			platform: "win32",
			// Claude Code's npm vendor dirs use `<arch>-win32`. cli.js's runtime
			// resolver inspects `process.platform === 'win32'` and picks `x64-win32`.
			ccVendorArch: "x64-win32",
			// Codex's Windows package follows Node's `process.platform` value
			// ("win32"), not the human-friendly "windows" naming.
			codexPkg: "@openai/codex-win32-x64",
			codexTriple: "x86_64-pc-windows-msvc",
			ghArch: "amd64",
			glabArch: "amd64",
			exeSuffix: ".exe",
			ghShaKey: "win_amd64",
			glabShaKey: "win_amd64",
		};
	}

	throw new Error(
		`[stage-vendor] Unsupported host platform ${process.platform}/${arch}. Add a branch in detectTarget().`,
	);
}

const target = detectTarget();
const isWin = target.platform === "win32";

// ---------------------------------------------------------------------------
// Copy helpers
// ---------------------------------------------------------------------------

function ensureExists(path: string, label: string): void {
	if (!existsSync(path)) {
		throw new Error(
			`[stage-vendor] expected ${label} at ${path} — run \`bun install\` in sidecar/ first`,
		);
	}
}

function copyFile(src: string, dest: string): void {
	mkdirSync(dirname(dest), { recursive: true });
	cpSync(src, dest);
}

function copyDir(src: string, dest: string): void {
	mkdirSync(dirname(dest), { recursive: true });
	cpSync(src, dest, { recursive: true });
}

function chmodExecutable(path: string): void {
	if (isWin) return; // Windows uses ACLs; .exe is executable by file extension
	chmodSync(path, 0o755);
}

function humanSize(path: string): string {
	if (!existsSync(path)) return "(missing)";
	let bytes = 0;
	const walk = (p: string): void => {
		const s = statSync(p);
		if (s.isDirectory()) {
			for (const entry of readdirSync(p)) {
				walk(join(p, entry));
			}
		} else if (s.isFile()) {
			bytes += s.size;
		}
	};
	walk(path);
	if (bytes > 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
	if (bytes > 1024) return `${(bytes / 1024).toFixed(1)} KB`;
	return `${bytes} B`;
}

// Shared entitlements plist — Bun's JSC JIT needs allow-jit +
// allow-unsigned-executable-memory under hardened runtime, otherwise
// spawn fails with "Ran out of executable memory while allocating N bytes".
// Windows skips this entirely.
const ENTITLEMENTS_PLIST = join(
	SIDECAR_ROOT,
	"..",
	"src-tauri",
	"Entitlements.plist",
);

// ---------------------------------------------------------------------------
// Forge CLI download (gh / glab) — pinned, cached at sidecar/.bundle-cache/
// ---------------------------------------------------------------------------

function ensureCacheDir(): void {
	mkdirSync(BUNDLE_CACHE, { recursive: true });
}

function sha256OfFile(path: string): string {
	// Use Node's crypto (synchronous, available on every Bun + Node runtime
	// without depending on Get-FileHash / shasum being on PATH).
	const { readFileSync } = require("node:fs") as typeof import("node:fs");
	const { createHash } = require("node:crypto") as typeof import("node:crypto");
	return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function downloadAndVerify(
	url: string,
	dest: string,
	expectedSha256: string,
): void {
	if (existsSync(dest)) {
		const actual = sha256OfFile(dest);
		if (actual === expectedSha256) return;
		if (SKIP_SHA_CHECK && expectedSha256.startsWith("PLACEHOLDER_")) {
			console.warn(
				`[stage-vendor] WINTHORPE_VENDOR_SKIP_SHA_CHECK=1 — accepting cached ${dest} (sha256: ${actual})`,
			);
			return;
		}
		console.warn(
			`[stage-vendor] cached ${dest} has wrong sha256 (got ${actual}); re-downloading`,
		);
		rmSync(dest, { force: true });
	}
	console.log(`[stage-vendor] downloading ${url}`);
	mkdirSync(dirname(dest), { recursive: true });
	if (isWin) {
		// curl ships with Windows 10+; PowerShell Invoke-WebRequest is the fallback.
		const curl = "curl.exe";
		execFileSync(curl, ["-fL", "--retry", "3", "-o", dest, url], {
			stdio: "inherit",
		});
	} else {
		execFileSync("curl", ["-fL", "--retry", "3", "-o", dest, url], {
			stdio: "inherit",
		});
	}
	const actual = sha256OfFile(dest);
	if (actual !== expectedSha256) {
		if (SKIP_SHA_CHECK && expectedSha256.startsWith("PLACEHOLDER_")) {
			console.warn(
				`[stage-vendor] WINTHORPE_VENDOR_SKIP_SHA_CHECK=1 — bootstrap mode\n` +
					`  Downloaded ${dest}\n` +
					`  SHA256: ${actual}\n` +
					`  Update stage-vendor.ts with this value and re-run without the env var.\n`,
			);
			return;
		}
		rmSync(dest, { force: true });
		throw new Error(
			`[stage-vendor] sha256 mismatch for ${url}\n  expected: ${expectedSha256}\n  actual:   ${actual}`,
		);
	}
}

// Wipe + recreate so a half-failed previous extract can never poison this run.
function freshExtractDir(path: string): void {
	rmSync(path, { recursive: true, force: true });
	mkdirSync(path, { recursive: true });
}

function extractArchive(archive: string, extractDir: string): void {
	if (isWin) {
		// Use built-in bsdtar (Windows 10+ ships it at System32\tar.exe) for
		// both .zip and .tar.gz. We pin to the absolute path so Git Bash
		// PATH order doesn't accidentally invoke MSYS2's GNU tar (which
		// can't read zip archives).
		const winTar = "C:\\Windows\\System32\\tar.exe";
		const tarBin = existsSync(winTar) ? winTar : "tar.exe";
		execFileSync(tarBin, ["-xf", archive, "-C", extractDir], {
			stdio: "inherit",
		});
	} else if (archive.endsWith(".zip")) {
		execFileSync("unzip", ["-q", "-o", archive, "-d", extractDir], {
			stdio: "inherit",
		});
	} else {
		execFileSync("tar", ["-xzf", archive, "-C", extractDir], {
			stdio: "inherit",
		});
	}
}

function stageGhBinary(): string {
	ensureCacheDir();
	const arch = target.ghArch;
	let slug: string;
	let archiveExt: string;
	if (target.platform === "darwin") {
		slug = `gh_${GH_VERSION}_macOS_${arch}`;
		archiveExt = "zip";
	} else {
		slug = `gh_${GH_VERSION}_windows_${arch}`;
		archiveExt = "zip";
	}
	const archive = join(BUNDLE_CACHE, `${slug}.${archiveExt}`);
	const url = `https://github.com/cli/cli/releases/download/v${GH_VERSION}/${slug}.${archiveExt}`;
	downloadAndVerify(url, archive, GH_SHA256[target.ghShaKey]);

	const extractDir = join(BUNDLE_CACHE, slug);
	freshExtractDir(extractDir);
	extractArchive(archive, extractDir);

	const binName = `gh${target.exeSuffix}`;
	const binSrc = locateExtractedBin(extractDir, binName);
	const binDest = join(DIST_VENDOR, "gh", binName);
	copyFile(binSrc, binDest);
	chmodExecutable(binDest);
	maybeSignMacBinary(binDest, false);
	return binDest;
}

/// Find `bin/<name>` either at the archive root or one wrapper level deep.
function locateExtractedBin(extractDir: string, name: string): string {
	const direct = join(extractDir, "bin", name);
	if (existsSync(direct)) return direct;
	for (const entry of readdirSync(extractDir)) {
		const nested = join(extractDir, entry, "bin", name);
		if (existsSync(nested)) return nested;
	}
	throw new Error(
		`[stage-vendor] could not locate bin/${name} under ${extractDir}`,
	);
}

function stageGlabBinary(): string {
	ensureCacheDir();
	const arch = target.glabArch;
	let slug: string;
	let archiveExt: string;
	if (target.platform === "darwin") {
		slug = `glab_${GLAB_VERSION}_darwin_${arch}`;
		archiveExt = "tar.gz";
	} else {
		slug = `glab_${GLAB_VERSION}_windows_${arch}`;
		archiveExt = "zip";
	}
	const archive = join(BUNDLE_CACHE, `${slug}.${archiveExt}`);
	const url = `https://gitlab.com/gitlab-org/cli/-/releases/v${GLAB_VERSION}/downloads/${slug}.${archiveExt}`;
	downloadAndVerify(url, archive, GLAB_SHA256[target.glabShaKey]);

	const extractDir = join(BUNDLE_CACHE, slug);
	freshExtractDir(extractDir);
	extractArchive(archive, extractDir);

	const binName = `glab${target.exeSuffix}`;
	// glab tarball: bin/glab at root. Windows zip: same layout.
	let binSrc = join(extractDir, "bin", binName);
	if (!existsSync(binSrc)) {
		// Some glab releases nest one level deep; try locateExtractedBin.
		binSrc = locateExtractedBin(extractDir, binName);
	}
	const binDest = join(DIST_VENDOR, "glab", binName);
	copyFile(binSrc, binDest);
	chmodExecutable(binDest);
	maybeSignMacBinary(binDest, false);
	return binDest;
}

function maybeSignMacBinary(path: string, withEntitlements: boolean): void {
	if (isWin) return; // Phase 8 wires Authenticode signing for Windows artifacts.
	const identity = process.env.APPLE_SIGNING_IDENTITY?.trim();
	if (!identity) return;

	const args = [
		"--force",
		"--sign",
		identity,
		"--timestamp",
		"--options",
		"runtime",
	];
	if (withEntitlements) {
		if (!existsSync(ENTITLEMENTS_PLIST)) {
			throw new Error(
				`[stage-vendor] Entitlements.plist missing at ${ENTITLEMENTS_PLIST}`,
			);
		}
		args.push("--entitlements", ENTITLEMENTS_PLIST);
	}
	args.push(path);

	console.log(
		`[stage-vendor] signing ${path}${withEntitlements ? " (+entitlements)" : ""}`,
	);
	execFileSync("codesign", args, { stdio: "inherit" });
}

// ---------------------------------------------------------------------------
// Bun host binary discovery
// ---------------------------------------------------------------------------

function locateHostBun(): string {
	try {
		if (isWin) {
			// `where` returns one or more paths; take the first.
			const raw = execSync("where bun", { encoding: "utf8" })
				.trim()
				.split(/\r?\n/)[0];
			if (!raw) throw new Error("empty output");
			// Windows symlinks are rare for Bun installs (winget/scoop/manual all
			// drop the real exe); skip realpathSync to avoid touching symlink
			// edge cases that the Node API doesn't always traverse.
			return raw;
		}
		const raw =
			execSync("which bun", { encoding: "utf8" }).trim().split("\n")[0] ?? "";
		if (!raw) throw new Error("empty output");
		// Homebrew ships bun as a symlink; resolve to the real Mach-O.
		return realpathSync(raw);
	} catch {
		throw new Error(
			"[stage-vendor] bun not found on PATH — install Bun (https://bun.sh) on the build host. " +
				"The Claude Agent SDK needs a JS runtime to execute cli.js, and bundle artifacts cannot rely " +
				"on the user's PATH. Winthorpe ships the host's bun binary inside the Resources/vendor/bun/ folder.",
		);
	}
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

console.log(
	`[stage-vendor] host=${process.platform}/${process.arch} ccArch=${target.ccVendorArch} codexPkg=${target.codexPkg}`,
);

// Clean
rmSync(DIST_VENDOR, { recursive: true, force: true });
mkdirSync(DIST_VENDOR, { recursive: true });

// ----- Claude Code -----
const ccSrc = join(NODE_MODULES, "@anthropic-ai/claude-code");
const ccDest = join(DIST_VENDOR, "claude-code");
ensureExists(join(ccSrc, "cli.js"), "@anthropic-ai/claude-code/cli.js");

copyFile(join(ccSrc, "cli.js"), join(ccDest, "cli.js"));

// Host-arch subset of claude-code's vendor dirs. cli.js resolves these
// relative to itself at runtime; any missing subdir just disables that
// particular feature (ripgrep → /search, audio-capture → voice I/O).
const ccVendorSubdirs = ["ripgrep", "audio-capture"] as const;
for (const sub of ccVendorSubdirs) {
	const from = join(ccSrc, "vendor", sub, target.ccVendorArch);
	if (existsSync(from)) {
		copyDir(from, join(ccDest, "vendor", sub, target.ccVendorArch));
	}
}

// ----- Codex -----
// Codex npm package layout: <pkg>/vendor/<triple>/codex/codex(.exe)
// Windows ships three companion exes alongside codex.exe (command-runner,
// windows-sandbox-setup) — copy the whole codex/ subdir so they ride along.
const codexVendorDir = join(
	NODE_MODULES,
	target.codexPkg,
	"vendor",
	target.codexTriple,
	"codex",
);
const codexBinName = `codex${target.exeSuffix}`;
const codexSrc = join(codexVendorDir, codexBinName);
ensureExists(codexSrc, `${target.codexPkg} ${codexBinName} binary`);

const codexDestDir = join(DIST_VENDOR, "codex");
if (isWin) {
	// Copy the entire codex/ folder so the helper exes land next to codex.exe.
	copyDir(codexVendorDir, codexDestDir);
	chmodExecutable(join(codexDestDir, codexBinName));
} else {
	// macOS ships a single codex Mach-O — copy just that file.
	const codexDest = join(codexDestDir, codexBinName);
	copyFile(codexSrc, codexDest);
	chmodExecutable(codexDest);
	maybeSignMacBinary(codexDest, false);
}

// ----- Bun (JS runtime for cli.js) -----
const bunBinName = `bun${target.exeSuffix}`;
const bunSrc = locateHostBun();
const bunDest = join(DIST_VENDOR, "bun", bunBinName);
copyFile(bunSrc, bunDest);
chmodExecutable(bunDest);
maybeSignMacBinary(bunDest, true);

// On Windows, Bun ships as a single .exe — no companion files. On macOS the
// host bun is a single Mach-O binary too.

for (const rel of [
	join(
		ccDest,
		"vendor",
		"ripgrep",
		target.ccVendorArch,
		`rg${target.exeSuffix}`,
	),
	join(
		ccDest,
		"vendor",
		"audio-capture",
		target.ccVendorArch,
		"audio-capture.node",
	),
]) {
	if (existsSync(rel)) {
		maybeSignMacBinary(rel, false);
	}
}

// ----- gh + glab (forge CLIs) -----
stageGhBinary();
stageGlabBinary();

// ----- Summary -----
console.log(`[stage-vendor] ✓ staged → ${DIST_VENDOR}`);
console.log(`  claude-code ${humanSize(ccDest)}`);
console.log(`  codex       ${humanSize(join(DIST_VENDOR, "codex"))}`);
console.log(`  bun         ${humanSize(join(DIST_VENDOR, "bun"))}`);
console.log(`  gh          ${humanSize(join(DIST_VENDOR, "gh"))}`);
console.log(`  glab        ${humanSize(join(DIST_VENDOR, "glab"))}`);
