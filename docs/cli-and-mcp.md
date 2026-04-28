# Winthorpe CLI & MCP Server

Winthorpe ships a companion CLI inside the desktop app bundle. Release builds
install `winthorpe`; debug builds install `winthorpe-dev`. The terminal entrypoint
always points at the currently installed desktop app so CLI and desktop
versions stay aligned.

## Install

### Settings UI

Open the desktop app → Settings → Experimental → **Command Line Tool** → Install.
This installs a symlink to the app bundle's `winthorpe-cli`:

- Release build: `/usr/local/bin/winthorpe`
- Debug build: `/usr/local/bin/winthorpe-dev`

### Development

```bash
bun run dev:cli:build
./src-tauri/target/debug/winthorpe-cli cli-status
bun run dev:cli:install
winthorpe-dev cli-status
```

The debug build reads `~/winthorpe-dev/` — same database as `bun run dev`.

## CLI Usage

```bash
winthorpe data info
winthorpe repo list
winthorpe repo add /path/to/repo
winthorpe workspace list
winthorpe workspace show winthorpe/earth            # human-readable ref
winthorpe workspace new --repo winthorpe
winthorpe session list --workspace winthorpe/earth
winthorpe session new --workspace winthorpe/earth
winthorpe send --workspace winthorpe/earth "Refactor the auth module"
```

Debug builds use the same commands under `winthorpe-dev`.

`--json` on any command outputs machine-readable JSON. `--data-dir <path>` overrides the data directory.

### Workspace References

Most commands accept either a UUID or a `repo-name/directory-name` shorthand:

```bash
winthorpe workspace show 5508edf1-bc73-4c6e-9c3d-21de3eeb25be   # UUID
winthorpe workspace show ai-shipany-template/draco                 # shorthand
```

## MCP Server

Run `winthorpe mcp` (or `winthorpe-dev mcp` in debug) to start a stdio MCP server implementing JSON-RPC 2.0.

### Exposed Tools

| Tool | Description |
|------|-------------|
| `winthorpe_data_info` | Data directory and build mode |
| `winthorpe_repo_list` | List repositories |
| `winthorpe_repo_add` | Register a local Git repo |
| `winthorpe_workspace_list` | List workspaces by status |
| `winthorpe_workspace_show` | Workspace details |
| `winthorpe_workspace_create` | Create workspace |
| `winthorpe_session_list` | List sessions |
| `winthorpe_session_create` | Create session |
| `winthorpe_send` | Send prompt to AI agent |

### Register with Claude Code

```bash
claude mcp add winthorpe -- /usr/local/bin/winthorpe mcp
```

Verify: `claude mcp list`

### Register with Claude Desktop

Edit `~/Library/Application Support/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "winthorpe": {
      "command": "/usr/local/bin/winthorpe",
      "args": ["mcp"]
    }
  }
}
```

Restart Claude Desktop.

### Register with Cursor

Edit `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "winthorpe": {
      "command": "/usr/local/bin/winthorpe",
      "args": ["mcp"]
    }
  }
}
```

### Dev Mode

Use the debug entrypoint instead:

```bash
claude mcp add winthorpe-dev -- /usr/local/bin/winthorpe-dev mcp
```

## Testing the MCP Server

### MCP Inspector (Web UI)

```bash
npx @modelcontextprotocol/inspector -- ./src-tauri/target/debug/winthorpe-cli mcp
```

Opens a browser UI to browse tools, invoke them, and inspect protocol traffic.

### Terminal Inspector

```bash
npx @wong2/mcp-cli -- ./src-tauri/target/debug/winthorpe-cli mcp
```

### Manual (pipe JSON-RPC)

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
| ./src-tauri/target/debug/winthorpe-cli mcp
```
