#!/usr/bin/env node
/**
 * Bundle-binary staging script. Tauri invokes this via `beforeBuildCommand`.
 *
 * Steps:
 * 1. `cd sidecar && bun install --frozen-lockfile` (so CI runners have deps).
 * 2. `bun run build` — produces `sidecar/dist/winthorpe-sidecar` plus the
 *    `sidecar/dist/vendor/` tree that Tauri bundles as resources.
 * 3. `cargo build --bin winthorpe-cli --release --target <triple>` — produces
 *    the CLI companion binary that ships inside the desktop app bundle.
 * 4. Copy the compiled sidecar / CLI to target-suffixed names so Tauri's
 *    `externalBin` entries can find the artifacts they expect.
 *
 * Usage (from repo root):
 *   node scripts/prepare-sidecar.mjs
 *   bun scripts/prepare-sidecar.mjs      # equivalent, Tauri uses this form
 */
import { execFileSync, execSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const sidecarDir = resolve(repoRoot, "sidecar");
const dotnetSidecarDir = resolve(repoRoot, "sidecar-dotnet");
const srcTauriDir = resolve(repoRoot, "src-tauri");
const bundledBinDir = resolve(srcTauriDir, "target", "bundled");
const entitlementsPlist = resolve(repoRoot, "src-tauri", "Entitlements.plist");

function run(cmd, cwd) {
	console.log(`[prepare-sidecar] $ ${cmd} (cwd: ${cwd})`);
	execSync(cmd, { cwd, stdio: "inherit" });
}

// Pre-sign the compiled sidecar with JIT entitlements so Bun's JSC runtime
// can allocate executable memory under hardened runtime. Tauri may re-sign
// this binary during bundling, but codesign preserves the entitlements blob
// unless --entitlements is passed again with a different plist.
function signSidecarWithEntitlements(path) {
	const identity = process.env.APPLE_SIGNING_IDENTITY?.trim();
	if (!identity) {
		console.log(
			"[prepare-sidecar] APPLE_SIGNING_IDENTITY unset — skipping sidecar pre-sign (dev / unsigned build)",
		);
		return;
	}
	if (!existsSync(entitlementsPlist)) {
		throw new Error(
			`[prepare-sidecar] Entitlements.plist missing at ${entitlementsPlist}`,
		);
	}
	console.log(`[prepare-sidecar] codesign (+entitlements) ${path}`);
	execFileSync(
		"codesign",
		[
			"--force",
			"--sign",
			identity,
			"--timestamp",
			"--options",
			"runtime",
			"--entitlements",
			entitlementsPlist,
			path,
		],
		{ stdio: "inherit" },
	);
}

function detectTargetTriple() {
	for (const key of [
		"TAURI_TARGET_TRIPLE",
		"TAURI_ENV_TARGET_TRIPLE",
		"CARGO_BUILD_TARGET",
	]) {
		const override = process.env[key]?.trim();
		if (override) {
			return override;
		}
	}
	const output = execSync("rustc --print host-tuple", {
		encoding: "utf8",
	}).trim();
	if (!output) {
		throw new Error("`rustc --print host-tuple` returned empty output");
	}
	return output;
}

function main() {
	// 1. Install sidecar deps (idempotent; fast when lockfile matches).
	run("bun install --frozen-lockfile", sidecarDir);

	// 2. Build the compiled Bun sidecar + staged vendor tree.
	run("bun run build", sidecarDir);

	// 2b. Build + AOT-publish the .NET sidecar (sub-host for C# user skills).
	//     Skipped if the dotnet SDK isn't on PATH — the .NET sidecar is
	//     opt-in; users without .NET installed get the Bun-only experience.
	maybeBuildDotnetSidecar();

	const triple = detectTargetTriple();
	const sidecarSource = resolve(sidecarDir, "dist", "winthorpe-sidecar");
	const sidecarDestination = resolve(
		sidecarDir,
		"dist",
		`winthorpe-sidecar-${triple}`,
	);
	const cliBinaryName =
		process.platform === "win32" ? "winthorpe-cli.exe" : "winthorpe-cli";
	const cliSource = resolve(
		srcTauriDir,
		"target",
		triple,
		"release",
		cliBinaryName,
	);
	const cliDestination = resolve(bundledBinDir, `winthorpe-cli-${triple}`);

	if (!existsSync(sidecarSource)) {
		throw new Error(
			`[prepare-sidecar] expected compiled sidecar at ${sidecarSource} but it does not exist`,
		);
	}

	// Tauri validates every `externalBin` during `cargo build`, including the
	// sidecar companion. Stage the target-suffixed sidecar first so a clean CI
	// checkout can compile `winthorpe-cli` without depending on stale artifacts.
	copyFileSync(sidecarSource, sidecarDestination);

	run(
		`cargo build --manifest-path ${resolve(srcTauriDir, "Cargo.toml")} --bin winthorpe-cli --release --target ${triple}`,
		repoRoot,
	);

	mkdirSync(bundledBinDir, { recursive: true });

	if (!existsSync(cliSource)) {
		throw new Error(
			`[prepare-sidecar] expected compiled CLI at ${cliSource} but it does not exist`,
		);
	}

	copyFileSync(cliSource, cliDestination);

	// Sign the target-suffixed copy (the one Tauri ingests as externalBin).
	// No-op when APPLE_SIGNING_IDENTITY is unset.
	signSidecarWithEntitlements(sidecarDestination);

	console.log(`[prepare-sidecar] staged sidecar → ${sidecarDestination}`);
	console.log(`[prepare-sidecar] staged CLI → ${cliDestination}`);
}

function maybeBuildDotnetSidecar() {
	if (!existsSync(dotnetSidecarDir)) {
		console.log(
			`[prepare-sidecar] sidecar-dotnet/ not present; skipping .NET sub-host build`,
		);
		return;
	}

	// Skip if dotnet SDK isn't installed. Don't fail — .NET is opt-in.
	let dotnetVersion;
	try {
		dotnetVersion = execSync("dotnet --version", { encoding: "utf8" }).trim();
	} catch {
		console.warn(
			`[prepare-sidecar] dotnet SDK not on PATH; skipping .NET sub-host build`,
		);
		return;
	}
	console.log(`[prepare-sidecar] dotnet SDK: ${dotnetVersion}`);

	// Map Cargo's target triple to .NET's runtime identifier.
	const triple = detectTargetTriple();
	const rid = ridForTriple(triple);
	if (!rid) {
		console.warn(
			`[prepare-sidecar] no dotnet RID mapping for triple ${triple}; skipping .NET sub-host build`,
		);
		return;
	}

	run(
		`dotnet publish -c Release -r ${rid} --no-self-contained false /p:PublishAot=true`,
		dotnetSidecarDir,
	);

	const exeName =
		process.platform === "win32"
			? "winthorpe-dotnet-sidecar.exe"
			: "winthorpe-dotnet-sidecar";
	const publishedExe = resolve(
		dotnetSidecarDir,
		"bin",
		"Release",
		"net10.0",
		rid,
		"publish",
		exeName,
	);
	if (!existsSync(publishedExe)) {
		throw new Error(
			`[prepare-sidecar] expected published .NET sidecar at ${publishedExe} but it does not exist`,
		);
	}

	// Stage next to the Bun sidecar so Tauri's bundler picks it up via the
	// same `../sidecar/dist/` resources directory.
	const dest = resolve(sidecarDir, "dist", exeName);
	copyFileSync(publishedExe, dest);
	console.log(`[prepare-sidecar] staged .NET sidecar → ${dest}`);
}

function ridForTriple(triple) {
	if (triple === "x86_64-pc-windows-msvc") return "win-x64";
	if (triple === "aarch64-pc-windows-msvc") return "win-arm64";
	if (triple === "x86_64-apple-darwin") return "osx-x64";
	if (triple === "aarch64-apple-darwin") return "osx-arm64";
	if (triple === "x86_64-unknown-linux-gnu") return "linux-x64";
	if (triple === "aarch64-unknown-linux-gnu") return "linux-arm64";
	return null;
}

main();
