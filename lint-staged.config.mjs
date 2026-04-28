// lint-staged config — controls which files trigger which commands and
// whether the staged file list is appended to each command.
//
// We use the function form so we can:
//   - Suppress the auto-appended file list for cargo commands. cargo clippy
//     and cargo fmt always operate on the whole crate; passing per-file
//     paths after `--` makes rustc treat them as additional input files
//     and fail with "multiple input filenames provided".
//   - Run biome on the staged JS/TS files directly (where per-file passes
//     are the desired behavior).

export default {
  "*.{ts,tsx,js,jsx,json,css}": (files) =>
    `biome check --write --error-on-warnings --no-errors-on-unmatched --files-ignore-unknown=true --config-path=./biome.json ${files.join(" ")}`,
  // Returning from a function (not a string array) is what suppresses
  // lint-staged's auto-append of file paths — without that, cargo clippy
  // forwards the staged .rs paths to rustc as additional input files and
  // fails with "multiple input filenames provided".
  //
  // The clippy command lives in scripts/lint-rust.sh so the lint allow-list
  // (mac-only dead code, doc-overindented inherited from upstream, etc.)
  // can be bumped without touching the lint-staged config.
  "*.rs": () => [
    "cargo fmt --manifest-path src-tauri/Cargo.toml --all --",
    "bash scripts/lint-rust.sh",
  ],
};
