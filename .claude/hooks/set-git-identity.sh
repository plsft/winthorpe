#!/bin/bash
# SessionStart hook: set git commit identity from per-user env vars.
#
# Each contributor can configure their own commit identity for Claude Code
# sessions on this repo by setting WINTHORPE_GIT_AUTHOR_NAME and
# WINTHORPE_GIT_AUTHOR_EMAIL as environment variables in THEIR claude.ai/code
# web environment (Settings → Environments → winthorpe → Environment
# variables). The values they set there are injected into their web
# sessions only — other contributors' sessions don't see them.
#
# If the env vars are unset (e.g. a contributor hasn't configured them yet,
# or this is a fresh local CLI session), the hook is a no-op and falls
# back to whatever git identity the environment already has.
#
# We deliberately use WINTHORPE_GIT_AUTHOR_* rather than the standard
# GIT_AUTHOR_NAME / GIT_AUTHOR_EMAIL env vars: those git natives would
# break GPG/SSH commit signature verification when set in web sandboxes
# (documented in anthropics/claude-code#18715). Writing via `git config`
# updates .git/config instead, which is safe.

if [ -n "$WINTHORPE_GIT_AUTHOR_NAME" ]; then
	git config user.name "$WINTHORPE_GIT_AUTHOR_NAME" || true
fi
if [ -n "$WINTHORPE_GIT_AUTHOR_EMAIL" ]; then
	git config user.email "$WINTHORPE_GIT_AUTHOR_EMAIL" || true
fi

exit 0
