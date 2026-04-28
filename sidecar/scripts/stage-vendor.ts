// Stage claude-code + codex + bun + gh + glab into `sidecar/dist/vendor/`
// for Tauri to ship as bundle resources. macOS host only.

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
//   gh:   github.com/cli/cli/releases/download/v$VER/gh_${VER}_checksums.txt
//   glab: gitlab.com/gitlab-org/cli/-/releases/v$VER/downloads/checksums.txt
const GH_VERSION = "2.91.0";
const GH_SHA256 = {
	arm64: "20446cd714d9fa1b69fbd410deade3731f38fe09a2b980c8488aa388dd320ada",
	amd64: "8806784f93603fe6d3f95c3583a08df38f175df9ebc123dc8b15f919329980e2",
} as const;

const GLAB_VERSION = "1.93.0";
const GLAB_SHA256 = {
	arm64: "6d6ffa97d430b5e7ff912e64dbac14703acc57967df654be1950ae71858d5b6f",
	amd64: "79d1a4f933919689c5fb7774feb1dd08f30b9c896dff4283b4a7387689ee0531",
} as const;

// ---------------------------------------------------------------------------
// Platform detection — macOS only, arch varies (arm64 / x64)
// ---------------------------------------------------------------------------

type NodeArch = "arm64" | "x64";

interface TargetInfo {
	/** `@anthropic-ai/claude-code` uses `<arch>-darwin` naming. */
	ccVendorArch: string;
	/** `@openai/codex-darwin-<arch>` is the npm optional-dep package. */
	codexPkg: string;
	/** Target triple used as the subdir inside the codex platform package. */
	codexTriple: string;
	/** `gh` release uses `arm64` / `amd64`. */
	ghArch: "arm64" | "amd64";
	/** `glab` release uses `arm64` / `amd64`. */
	glabArch: "arm64" | "amd64";
}

function detectTarget(): TargetInfo {
	if (process.platform !== "darwin") {
		throw new Error(
			`[stage-vendor] Winthorpe only builds on macOS; host platform is ${process.platform}`,
		);
	}
	const arch = process.arch as NodeArch;

	switch (arch) {
		case "arm64":
			return {
				ccVendorArch: "arm64-darwin",
				codexPkg: "@openai/codex-darwin-arm64",
				codexTriple: "aarch64-apple-darwin",
				ghArch: "arm64",
				glabArch: "arm64",
			};
		case "x64":
			return {
				ccVendorArch: "x64-darwin",
				codexPkg: "@openai/codex-darwin-x64",
				codexTriple: "x86_64-apple-darwin",
				ghArch: "amd64",
				glabArch: "amd64",
			};
		default:
			throw new Error(`[stage-vendor] Unsupported macOS arch: ${arch}`);
	}
}

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
	const out = execFileSync("shasum", ["-a", "256", path], {
		encoding: "utf8",
	});
	const digest = out.split(/\s+/)[0];
	if (!digest) throw new Error(`[stage-vendor] empty shasum for ${path}`);
	return digest;
}

function downloadAndVerify(
	url: string,
	dest: string,
	expectedSha256: string,
): void {
	if (existsSync(dest)) {
		const actual = sha256OfFile(dest);
		if (actual === expectedSha256) return;
		console.warn(
			`[stage-vendor] cached ${dest} has wrong sha256 (got ${actual}); re-downloading`,
		);
		rmSync(dest, { force: true });
	}
	console.log(`[stage-vendor] downloading ${url}`);
	mkdirSync(dirname(dest), { recursive: true });
	execFileSync("curl", ["-fL", "--retry", "3", "-o", dest, url], {
		stdio: "inherit",
	});
	const actual = sha256OfFile(dest);
	if (actual !== expectedSha256) {
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

function stageGhBinary(arch: "arm64" | "amd64"): string {
	ensureCacheDir();
	const slug = `gh_${GH_VERSION}_macOS_${arch}`;
	const archive = join(BUNDLE_CACHE, `${slug}.zip`);
	const url = `https://github.com/cli/cli/releases/download/v${GH_VERSION}/${slug}.zip`;
	downloadAndVerify(url, archive, GH_SHA256[arch]);

	// Unzip into a dedicated temp dir, then locate `bin/gh` regardless of
	// whether the archive carries an internal wrapper directory. We strip
	// the wrapper after the fact so changes upstream (with or without it)
	// don't silently leave stale files in BUNDLE_CACHE.
	const extractDir = join(BUNDLE_CACHE, slug);
	freshExtractDir(extractDir);
	execFileSync("unzip", ["-q", "-o", archive, "-d", extractDir], {
		stdio: "inherit",
	});

	const binSrc = locateExtractedBin(extractDir, "gh");
	const binDest = join(DIST_VENDOR, "gh", "gh");
	copyFile(binSrc, binDest);
	chmodSync(binDest, 0o755);
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

function stageGlabBinary(arch: "arm64" | "amd64"): string {
	ensureCacheDir();
	const slug = `glab_${GLAB_VERSION}_darwin_${arch}`;
	const archive = join(BUNDLE_CACHE, `${slug}.tar.gz`);
	const url = `https://gitlab.com/gitlab-org/cli/-/releases/v${GLAB_VERSION}/downloads/${slug}.tar.gz`;
	downloadAndVerify(url, archive, GLAB_SHA256[arch]);

	// glab's tarball has no wrapper dir; bin/glab is at the archive root.
	const extractDir = join(BUNDLE_CACHE, slug);
	freshExtractDir(extractDir);
	execFileSync("tar", ["-xzf", archive, "-C", extractDir], {
		stdio: "inherit",
	});

	const binSrc = join(extractDir, "bin", "glab");
	if (!existsSync(binSrc)) {
		throw new Error(
			`[stage-vendor] glab binary missing after extract: ${binSrc}`,
		);
	}
	const binDest = join(DIST_VENDOR, "glab", "glab");
	copyFile(binSrc, binDest);
	chmodSync(binDest, 0o755);
	maybeSignMacBinary(binDest, false);
	return binDest;
}

function maybeSignMacBinary(path: string, withEntitlements: boolean): void {
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
// Main
// ---------------------------------------------------------------------------

const target = detectTarget();

console.log(
	`[stage-vendor] host=darwin/${process.arch} ccArch=${target.ccVendorArch} codexPkg=${target.codexPkg}`,
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
const codexSrc = join(
	NODE_MODULES,
	target.codexPkg,
	"vendor",
	target.codexTriple,
	"codex",
	"codex",
);
ensureExists(codexSrc, `${target.codexPkg} codex binary`);

const codexDest = join(DIST_VENDOR, "codex", "codex");
copyFile(codexSrc, codexDest);
chmodSync(codexDest, 0o755);
maybeSignMacBinary(codexDest, false);

// ----- Bun (JS runtime for cli.js) -----
function locateHostBun(): string {
	try {
		const raw =
			execSync("which bun", { encoding: "utf8" }).trim().split("\n")[0] ?? "";
		if (!raw) throw new Error("empty output");
		// Homebrew ships bun as a symlink; resolve to the real Mach-O.
		return realpathSync(raw);
	} catch {
		throw new Error(
			"[stage-vendor] bun not found on PATH — install Bun (https://bun.sh) on the build host. " +
				"The Claude Agent SDK needs a JS runtime to execute cli.js, and `.app` bundles cannot rely " +
				"on the user's PATH. We ship the host's bun binary inside Winthorpe.app/Contents/Resources/vendor/bun/.",
		);
	}
}

const bunSrc = locateHostBun();
const bunDest = join(DIST_VENDOR, "bun", "bun");
copyFile(bunSrc, bunDest);
chmodSync(bunDest, 0o755);
maybeSignMacBinary(bunDest, true);

for (const rel of [
	join(ccDest, "vendor", "ripgrep", target.ccVendorArch, "rg"),
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
stageGhBinary(target.ghArch);
stageGlabBinary(target.glabArch);

// ----- Summary -----
console.log(`[stage-vendor] ✓ staged → ${DIST_VENDOR}`);
console.log(`  claude-code ${humanSize(ccDest)}`);
console.log(`  codex       ${humanSize(join(DIST_VENDOR, "codex"))}`);
console.log(`  bun         ${humanSize(join(DIST_VENDOR, "bun"))}`);
console.log(`  gh          ${humanSize(join(DIST_VENDOR, "gh"))}`);
console.log(`  glab        ${humanSize(join(DIST_VENDOR, "glab"))}`);
