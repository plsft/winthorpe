// JSON-RPC message types for the .NET sidecar protocol.
//
// All types are records so System.Text.Json source-generator (see
// SidecarJsonContext) can produce AOT-friendly serializers — no reflection
// at runtime, which matches the AOT publish settings in the .csproj.

using System.Text.Json;
using System.Text.Json.Serialization;

namespace Winthorpe.Sidecar;

/// <summary>
/// Initial handshake frame emitted before the request loop starts.
/// The Rust parent reads exactly one of these and validates `type == "ready"`.
/// </summary>
public sealed record ReadyFrame(
    [property: JsonPropertyName("type")] string Type,
    [property: JsonPropertyName("runtime")] string Runtime,
    [property: JsonPropertyName("capabilities")] string[] Capabilities);

/// <summary>
/// Inbound JSON-RPC request from the Bun sidecar (or Rust directly).
/// </summary>
public sealed record Request(
    [property: JsonPropertyName("id")] string Id,
    [property: JsonPropertyName("method")] string Method,
    [property: JsonPropertyName("params")] JsonElement? Params);

/// <summary>
/// Successful response. `Result` is an opaque JsonElement so dispatchers
/// don't need to commit to a per-method shape at the protocol layer.
/// </summary>
public sealed record Response(
    [property: JsonPropertyName("id")] string Id,
    [property: JsonPropertyName("result")] JsonElement Result);

public sealed record ErrorResponse(
    [property: JsonPropertyName("id")] string? Id,
    [property: JsonPropertyName("error")] ErrorBody Error);

public sealed record ErrorBody(
    [property: JsonPropertyName("code")] int Code,
    [property: JsonPropertyName("message")] string Message);

/// <summary>
/// Source-generated JSON serializers — AOT-clean, reflection-free.
/// </summary>
[JsonSerializable(typeof(ReadyFrame))]
[JsonSerializable(typeof(Request))]
[JsonSerializable(typeof(Response))]
[JsonSerializable(typeof(ErrorResponse))]
[JsonSerializable(typeof(ErrorBody))]
[JsonSourceGenerationOptions(
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull)]
public partial class SidecarJsonContext : JsonSerializerContext
{
}
