import fs from "node:fs";
import path from "node:path";

const root = process.cwd();
const packageJsonPath = path.join(root, "package.json");
const cargoTomlPath = path.join(root, "src-tauri", "Cargo.toml");
const cargoLockPath = path.join(root, "src-tauri", "Cargo.lock");
const tauriConfigPath = path.join(root, "src-tauri", "tauri.conf.json");

const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, "utf8"));
const version = packageJson.version;

if (!version) {
	throw new Error("package.json version is missing");
}

const cargoToml = fs.readFileSync(cargoTomlPath, "utf8");
const nextCargoToml = cargoToml.replace(
	/^version = ".*"$/m,
	`version = "${version}"`,
);

if (cargoToml === nextCargoToml) {
	console.log(`Cargo.toml already matches ${version}`);
} else {
	fs.writeFileSync(cargoTomlPath, nextCargoToml);
	console.log(`Updated src-tauri/Cargo.toml to ${version}`);
}

// Cargo.lock keeps a `[[package]]` entry for the winthorpe crate with its own
// version field. CI never runs cargo, so without this step the lockfile
// drifts and everyone regenerates it locally via rust-analyzer.
const cargoLock = fs.readFileSync(cargoLockPath, "utf8");
// CRLF-aware: Cargo.lock is committed with the developer's line endings,
// which on Windows (where this repo's primary contributor works) means
// "\r\n". Match either to keep this script portable across platforms.
const cargoLockPattern = /(^name = "winthorpe"\r?\nversion = )"[^"]*"/m;
if (!cargoLockPattern.test(cargoLock)) {
	throw new Error(
		'src-tauri/Cargo.lock is missing the `name = "winthorpe"` package entry',
	);
}
const nextCargoLock = cargoLock.replace(cargoLockPattern, `$1"${version}"`);

if (cargoLock === nextCargoLock) {
	console.log(`Cargo.lock already matches ${version}`);
} else {
	fs.writeFileSync(cargoLockPath, nextCargoLock);
	console.log(`Updated src-tauri/Cargo.lock to ${version}`);
}

const tauriConfig = JSON.parse(fs.readFileSync(tauriConfigPath, "utf8"));
tauriConfig.version = version;
fs.writeFileSync(
	tauriConfigPath,
	`${JSON.stringify(tauriConfig, null, "\t")}\n`,
);
console.log(`Updated src-tauri/tauri.conf.json to ${version}`);
