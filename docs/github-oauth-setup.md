# GitHub OAuth setup

Winthorpe's "Connect GitHub" flow uses GitHub's [Device Flow][df] OAuth
grant. You create your own GitHub OAuth App once and paste its client ID
into a local `.env.local` so the in-app sign-in authenticates against
**your** OAuth App.

The repo intentionally ships with an invalid placeholder client ID
(`REPLACE_WITH_YOUR_GITHUB_OAUTH_CLIENT_ID`) so a fresh build that
hasn't been configured fails loudly rather than silently authenticating
against someone else's app.

[df]: https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow

## One-time setup

### 1. Create the OAuth App

Go to one of:
- **Personal** (recommended for solo use): https://github.com/settings/developers
- **Organization** — your org's developer settings page

Click **New OAuth App** and fill in:

| Field | Value |
|---|---|
| Application name | `Winthorpe` (or whatever you prefer) |
| Homepage URL | any valid URL — used in the GitHub OAuth UI only |
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

Replace the placeholder with the Client ID from step 3.

### 5. Rebuild

```pwsh
bun run dev:win
```

The build script in `src-tauri/build.rs` reads `.env.local` at compile
time and bakes the client ID in via `option_env!("WINTHORPE_GITHUB_CLIENT_ID")`.
The "Connect GitHub" flow in Settings will work end-to-end after this.

## Scopes Winthorpe requests at runtime

When the user signs in, Winthorpe asks GitHub for these scopes:

- `repo` — read/write to repos (fetch, branch, push, open PRs)
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

## Defense-in-depth

`src-tauri/src/forge/github/auth.rs::github_client_id()` rejects the
`REPLACE_WITH_YOUR_GITHUB_OAUTH_CLIENT_ID` placeholder at runtime, so
even an unconfigured build can't accidentally try to authenticate
against a non-existent (or misconfigured) OAuth app. The "Connect
GitHub" button surfaces a clear "not configured" error instead.
