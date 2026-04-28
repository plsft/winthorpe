# Winthorpe .NET sidecar

A small AOT-published .NET 10 binary that runs C# user skills on demand.

## Why this exists

Winthorpe's primary sidecar (`../sidecar/`) is a Bun process that hosts
LLM agent sessions (Claude Agent SDK + Codex SDK). When a user wants to
write skills, custom tools, or workspace automation in **C#** instead of
TypeScript, the Bun sidecar dispatches a `runSkill` request over stdin
to *this* process. We keep the LLM transport in Bun (mature SDKs) and
the user-code execution in .NET (idiomatic C#, real type system, .NET
ecosystem).

## Protocol

JSON-RPC over stdin/stdout, newline-delimited. Same shape as the Bun
sidecar:

```jsonc
// initial handshake (sidecar → parent)
{"type":"ready","runtime":"dotnet-10","capabilities":["runSkill","ping"]}

// request (parent → sidecar)
{"id":"req-1","method":"runSkill","params":{...}}

// response (sidecar → parent)
{"id":"req-1","result":{...}}

// error
{"id":"req-1","error":{"code":-32601,"message":"Method not found: foo"}}
```

## Build

```pwsh
# Dev / quick iteration (JIT, fastest build)
dotnet build -c Debug

# Release: native AOT, single-file exe, ~5 MB
dotnet publish -c Release -r win-x64
# → bin/Release/net10.0/win-x64/publish/winthorpe-dotnet-sidecar.exe
```

The Tauri bundler stages `winthorpe-dotnet-sidecar.exe` next to the Bun
sidecar (`scripts/prepare-sidecar.mjs` is updated in Phase 6 follow-up
to run the dotnet publish before stage-vendor copies the binary into
`sidecar/dist/`).

## Skill loading model

The current dispatcher returns a stub for `runSkill`. The real plan:

1. Bun sidecar sends `runSkill` with a `params.skillPath` pointing at
   either:
   - A `.csproj` to compile and load (development), or
   - A pre-published skill DLL/exe (production).
2. This sidecar shells out to `dotnet run --project <path>` (dev) or
   loads a published `*.exe` directly (production), using a fresh
   `Process` per call so user code can't pollute the host's AOT image.
3. stdout/stderr from the skill stream back as `ScriptEvent`-shaped
   events (`type: "stdout" | "stderr"`).

Per-call subprocess isolation is intentional: the host stays AOT (small
+ fast cold start), and arbitrary user assemblies never get loaded into
the long-lived sidecar process.
