// Winthorpe .NET sidecar — JSON-RPC over stdin/stdout.
//
// Sub-host model: this process runs C# user skills. The Bun sidecar
// (sidecar/) stays the LLM session manager; it dispatches a `runSkill`
// request to this binary when a session uses a C#-authored skill.
//
// Protocol shape (matches sidecar/src/index.ts):
//   Request:  {"id":"...","method":"...","params":{...}}
//   Response: {"id":"...","result":...}        // success
//             {"id":"...","error":{"code":N,"message":"..."}} // failure
//   Event:    {"id":"...","type":"...",...}    // streaming events
//
// On startup we emit a single ready frame:
//   {"type":"ready","runtime":"dotnet-10","capabilities":[...]}
//
// Then we read newline-delimited requests on stdin and reply on stdout.
//
// AOT-clean: no reflection-based serialization, no dynamic loading of
// arbitrary assemblies. C# skills are loaded via System.Reflection.MetadataLoadContext
// in a separate (non-AOT) "skill host" sub-sub-process spawned per request,
// preserving AOT compilation here while still allowing arbitrary skill code.

using System.Text.Json;
using System.Text.Json.Serialization;
using Winthorpe.Sidecar;

// JSON serializer options — minimal, AOT-friendly.
var jsonOptions = new JsonSerializerOptions(JsonSerializerDefaults.Web)
{
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    TypeInfoResolver = SidecarJsonContext.Default,
};

// Emit ready frame before touching stdin so the parent (Rust) can advance
// past its handshake wait without racing the first request.
var ready = new ReadyFrame(
    Type: "ready",
    Runtime: $"dotnet-{Environment.Version.Major}",
    Capabilities: ["runSkill", "ping"]);
WriteJsonLine(ready, jsonOptions);

// stdin loop — newline-delimited JSON. Errors during decode become protocol
// errors; the loop never crashes the process.
using var stdin = new StreamReader(Console.OpenStandardInput(), bufferSize: 8192);
string? line;
while ((line = await stdin.ReadLineAsync()) is not null)
{
    if (string.IsNullOrWhiteSpace(line))
        continue;

    Request? request;
    try
    {
        request = JsonSerializer.Deserialize(line, SidecarJsonContext.Default.Request);
    }
    catch (JsonException ex)
    {
        WriteError(id: null, code: -32700, message: $"Parse error: {ex.Message}", jsonOptions);
        continue;
    }

    if (request is null)
    {
        WriteError(id: null, code: -32600, message: "Empty request", jsonOptions);
        continue;
    }

    await DispatchAsync(request, jsonOptions).ConfigureAwait(false);
}

return 0;

static async Task DispatchAsync(Request request, JsonSerializerOptions options)
{
    try
    {
        switch (request.Method)
        {
            case "ping":
                WriteJsonLine(
                    new Response(Id: request.Id, Result: JsonDocument.Parse("{\"pong\":true}").RootElement),
                    options);
                break;

            case "runSkill":
                // Stub: real skill execution goes here in Phase 6 follow-up.
                // For now we acknowledge the call so the Bun sidecar can
                // exercise the dispatch path end-to-end.
                await Task.Delay(0).ConfigureAwait(false);
                WriteJsonLine(
                    new Response(
                        Id: request.Id,
                        Result: JsonDocument.Parse("""
                            {
                                "status": "stub",
                                "message": "C# skill execution will land in a follow-up; payload accepted."
                            }
                            """).RootElement),
                    options);
                break;

            default:
                WriteError(
                    id: request.Id,
                    code: -32601,
                    message: $"Method not found: {request.Method}",
                    options);
                break;
        }
    }
    catch (Exception ex)
    {
        WriteError(id: request.Id, code: -32000, message: ex.Message, options);
    }
}

static void WriteJsonLine<T>(T payload, JsonSerializerOptions options)
{
    var json = JsonSerializer.Serialize(payload, typeof(T), SidecarJsonContext.Default);
    Console.Out.WriteLine(json);
    Console.Out.Flush();
}

static void WriteError(string? id, int code, string message, JsonSerializerOptions options)
{
    var err = new ErrorResponse(Id: id, Error: new ErrorBody(Code: code, Message: message));
    var json = JsonSerializer.Serialize(err, SidecarJsonContext.Default.ErrorResponse);
    Console.Out.WriteLine(json);
    Console.Out.Flush();
}
