#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <Winthorpe.app path>"
  exit 1
fi

APP_BUNDLE="$1"
CLI_PATH="${APP_BUNDLE}/Contents/MacOS/winthorpe-cli"

if [[ ! -d "${APP_BUNDLE}" ]]; then
  echo "App bundle not found: ${APP_BUNDLE}"
  exit 1
fi

if [[ ! -x "${CLI_PATH}" ]]; then
  echo "Bundled CLI missing or not executable: ${CLI_PATH}"
  exit 1
fi

echo "Verifying bundled CLI at ${CLI_PATH}..."
OUTPUT="$("${CLI_PATH}" cli-status --json)"

if [[ -z "${OUTPUT}" ]]; then
  echo "Bundled CLI returned empty output"
  exit 1
fi

if [[ "${OUTPUT}" != *'"buildMode"'* ]] || [[ "${OUTPUT}" != *'"currentBinary"'* ]]; then
  echo "Bundled CLI did not return the expected cli-status JSON:"
  echo "${OUTPUT}"
  exit 1
fi

echo "Bundled CLI smoke check passed."
