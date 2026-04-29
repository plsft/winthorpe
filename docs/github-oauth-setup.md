# GitHub OAuth setup (plsft)

Winthorpe's "Connect GitHub" flow uses GitHub's [Device Flow][df] OAuth
grant. To make Winthorpe authenticate against your own plsft account
(rather than the upstream Helmor OAuth app), you create a personal /
organization OAuth App on GitHub once and paste its client ID into a
local `.env.local`.

[df]: https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow

## One-time setup

### 1. Create the OAuth App

Go to one of:
- **Personal** (recommended for solo use): https://github.com/settings/developers
- **Organization** (recommended for shared use under plsft):
  `https://github.com/organizations/plsft/settings/applications`

Click **New OAuth App** and fill in:

| Field | Value |
|---|---|
| Application name | `Winthorpe` (or whatever you prefer) |
| Homepage URL | `https://github.com/plsft/winthorpe` (or any URL — GitHub just needs *something*) |
| Application description | optional |
| Authorization callback URL | `http://localhost` — Device Flow doesn't actually call back, but GitHub requires the field to be non-empty |

Click **Register application**.

### 2. Enable Device Flow

After the app is created you'll land on its settings page. **Scroll
down to the "Device Flow" section and check "Enable Device Flow"**, then
click **Update application**.

Without this, the in-app "Connect GitHub" button will fail with an
"unsupported_grant_type" error.

### 3. Copy the Client ID

At the top of the OAuth app's settings page, you'll see:

```
Client ID
Ov23li...
```

Copy that value (it's not secret — it's safe to paste into config files
even if they're checked into a public repo, though `.env.local` is
gitignored anyway).

### 4. Wire it into Winthorpe

```pwsh
# From the repo root:
cp .env.local.example .env.local
notepad .env.local
```

Replace `PASTE_YOUR_PLSFT_CLIENT_ID_HERE` with the Client ID from step 3.

### 5. Rebuild

```pwsh
bun run dev:win
```

The build script in `src-tauri/build.rs` reads `.env.local` at compile
time and bakes the client ID in via `option_env!("WINTHORPE_GITHUB_CLIENT_ID")`.
You should see the "Connect GitHub" flow in Settings work end-to-end after this.

## Scopes Winthorpe requests at runtime

When the user signs in, Winthorpe asks GitHub for these scopes:

- `repo` — read/write to repos (so Winthorpe can fetch, branch, push, open PRs)
- `read:org` — list org-scoped repos
- `user:email` — show the signed-in user's verified primary email

These are requested at sign-in time — you do **not** configure them on the
OAuth app itself. (GitHub OAuth Apps don't have per-app scope restrictions
the way GitHub Apps do.)

Source: `src-tauri/src/forge/github/auth.rs::GITHUB_OAUTH_SCOPES`.

## Optional: client secret

Device Flow doesn't use a client secret, so leave the **Generate a new
client secret** button alone. If you accidentally generated one, it's
harmless — Winthorpe never sends it.

## Switching back to upstream Helmor's OAuth app

The repo's `.env.example` ships with the upstream Helmor client ID
(`Ov23lijdFdN5ZxRNSHah`) so a fresh checkout works without any setup. To
revert to it from a configured `.env.local`, either delete `.env.local`
or replace your `WINTHORPE_GITHUB_CLIENT_ID` value with that string.

(Using upstream's OAuth app is fine for casual local testing but means
the GitHub authorization screen says "Helmor" rather than "Winthorpe" —
which is why we recommend creating your own.)
