#!/usr/bin/env bash
# Run cargo clippy with project-specific lint allow-list.
#
# Allow-listed lints (rationale per item):
#
#   dead_code, unused_imports
#     Many helpers (CredentialStore, JobObject methods, install/deep_link
#     module functions) are intentionally exposed for the next iteration
#     and not yet wired into the live code path. Failing the build on
#     these would force ad-hoc #[allow(dead_code)] sprinkles everywhere
#     during port phases. Cleanup is tracked in docs/winthorpe-port.md.
#
#   clippy::doc_overindented_list_items
#     Triggers on existing upstream module-doc comments that use 5-space
#     bullet indentation (a personal style). Not worth a 100-file rewrite.
#
#   non_snake_case (test names containing camelCase fixture words like
#   `packageManager`)
#     We name tests after the JSON keys they assert against to keep the
#     test grep'able by feature.
#
#   unexpected_cfgs
#     The `legacy-pty` feature gate is documented in scripts/mod.rs as a
#     phase-out hatch; clippy doesn't see the matching feature definition
#     because it's intentionally not in Cargo.toml until needed.
set -e
cd "$(dirname "$0")/.."

# On Windows: hydrate MSVC env so cargo finds the right link.exe + LIB.
# Mirrors .husky/pre-commit so this script also works when invoked outside
# the hook (e.g. `bun run lint`).
case "$(uname -s 2>/dev/null || echo unknown)" in
  MINGW*|MSYS*|CYGWIN*)
    bat="$(pwd)/scripts/hydrate-msvc-env.bat"
    if [[ -f "$bat" ]]; then
      while IFS= read -r line; do
        line="${line%$'\r'}"
        case "$line" in
          INCLUDE=*|LIB=*|LIBPATH=*|VCToolsInstallDir=*|WindowsSdkDir=*|UCRTVersion=*|VCINSTALLDIR=*|VCToolsVersion=*|WindowsSDKVersion=*)
            export "$line"
            ;;
        esac
      done < <(cmd //c "$(cygpath -w "$bat")")
      if [[ -n "$VCToolsInstallDir" ]]; then
        msvc_bin_win="${VCToolsInstallDir%\\}\\bin\\HostX64\\x64"
        msvc_bin=$(cygpath -u "$msvc_bin_win")
        export PATH="$msvc_bin:$PATH"
      fi
    fi
    ;;
esac

cargo clippy \
    --manifest-path src-tauri/Cargo.toml \
    --all-targets \
    -- \
    -D warnings \
    -A dead_code \
    -A unused_imports \
    -A unused_variables \
    -A clippy::doc_overindented_list_items \
    -A non_snake_case \
    -A unexpected_cfgs \
    -A clippy::duplicated_attributes \
    -A clippy::needless_return
