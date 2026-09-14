// Cross-implementation differential execution runner (C# side).
//
// Mirrors neo-vm/examples/vm_diff_runner.rs: reads the same JSON vector file,
// executes each script on a bare ExecutionEngine, and writes the same result
// schema. See scripts/vm-diff.py.
//
// Usage:
//   dotnet run --project tools/csharp-vm-runner -- <in.json> <out.json>
//
// Pinned to Neo.VM 3.10.1 (the audit baseline). No host, no syscalls, no
// storage: vectors referencing syscalls must FAULT identically on both sides.
//
// The result stack is rendered with the same Neo JSON-RPC stack envelope shape
// the Rust side uses (Neo.VM's own ToJson via StackItem), so the comparison
// uses one canonical form both implementations already agree on.

using System.Text.Json;
using System.Text.Json.Nodes;
using Neo.VM;
using Neo.VM.Types;

namespace CSharpVmRunner;

internal static class Program
{
    // Independent per-item render budget, matching the Rust harness.
    private const int RpcStackBudget = 2 * 1024 * 1024;

    private static int Main(string[] args)
    {


        if (args.Length != 2)
        {
            Console.Error.WriteLine("usage: csharp-vm-runner <in.json> <out.json>");
            return 2;
        }

        var raw = File.ReadAllText(args[0]);
        using var doc = JsonDocument.Parse(raw);
        var vectors = doc.RootElement.GetProperty("vectors");

        var results = new JsonArray();
        foreach (var vector in vectors.EnumerateArray())
        {
            results.Add(RunOne(vector));
        }

        var output = new JsonObject
        {
            ["impl_name"] = "csharp-neo-vm-3.10.1",
            ["results"] = results,
        };

        File.WriteAllText(args[1], output.ToJsonString(new JsonSerializerOptions
        {
            WriteIndented = true,
        }));
        return 0;
    }

    private static JsonObject RunOne(JsonElement vector)
    {
        var name = vector.GetProperty("name").GetString() ?? "";
        var scriptHex = vector.GetProperty("script").GetString() ?? "";
        var rvcount = vector.TryGetProperty("rvcount", out var rv) && rv.ValueKind == JsonValueKind.Number
            ? rv.GetInt32()
            : -1;

        byte[] code;
        try
        {
            code = Convert.FromHexString(scriptHex);
        }
        catch (Exception ex)
        {
            return new JsonObject
            {
                ["name"] = name,
                ["state"] = "HARNESS_ERROR",
                ["stack"] = new JsonArray(),
                ["fault"] = null,
                ["harness_error"] = $"hex decode: {ex.Message}",
            };
        }

        var engine = new ExecutionEngine(null);
        string? fault = null;

        try
        {
            var script = new Script(code);
            engine.LoadScript(script, rvcount, 0);
            engine.Execute();

            // In Neo.VM the fault is surfaced as VMUnhandledException.
            if (engine.State == VMState.FAULT)
            {
                // Re-derive the fault detail: Neo.VM stores it on the engine.
                fault = DescribeFault(engine);
            }
        }
        catch (Exception ex)
        {
            fault = Classify($"{ex.GetType().Name}: {ex.Message}");
        }

        var stateName = engine.State switch
        {
            VMState.HALT => "HALT",
            VMState.FAULT => "FAULT",
            VMState.BREAK => "BREAK",
            _ => "NONE",
        };

        var stack = new JsonArray();
        string? harnessError = null;
        try
        {
            long budget = RpcStackBudget;
            foreach (var item in engine.ResultStack)
            {
                stack.Add(RenderItem(item, ref budget));
            }
        }
        catch (Exception ex)
        {
            harnessError = $"stack render failed: {ex.Message}";
        }

        return new JsonObject
        {
            ["name"] = name,
            ["state"] = stateName,
            ["stack"] = stack,
            ["fault"] = fault,
            ["harness_error"] = harnessError,
        };
    }

    /// <summary>
    /// Render a stack item in the shared Neo RPC envelope.
    ///
    /// Mirrors the Rust side's <c>stack_item_rpc_json</c> exactly:
    /// Integer -&gt; decimal string, ByteString/Buffer -&gt; base64,
    /// Array/Struct -&gt; nested list under "value", Map -&gt; key/value pairs.
    /// </summary>
    private static JsonNode RenderItem(StackItem item, ref long budget)
    {
        switch (item)
        {
            case Neo.VM.Types.Boolean b:
                return Node("Boolean", JsonValue.Create(b.GetBoolean()));
            case Integer i:
                return Node("Integer", JsonValue.Create(i.GetInteger().ToString()));
            case ByteString bs:
                return Node("ByteString", JsonValue.Create(Convert.ToBase64String(bs.GetSpan())));
            case Neo.VM.Types.Buffer buf:
                return Node("Buffer", JsonValue.Create(Convert.ToBase64String(buf.GetSpan())));
            case Null:
                return Node("Null", null);
            case Neo.VM.Types.Array arr:
            {
                var kind = item is Struct ? "Struct" : "Array";
                var items = new JsonArray();
                foreach (var child in arr)
                {
                    items.Add(RenderItem(child, ref budget));
                }
                return Node(kind, items);
            }
            case Map map:
            {
                var entries = new JsonArray();
                foreach (var (k, v) in map)
                {
                    entries.Add(new JsonObject
                    {
                        ["key"] = RenderItem(k, ref budget),
                        ["value"] = RenderItem(v, ref budget),
                    });
                }
                return Node("Map", entries);
            }
            case Pointer:
                return Node("Pointer", JsonValue.Create("0"));
            default:
                return Node("InteropInterface", null);
        }
    }

    private static JsonObject Node(string type, JsonNode? value)
    {
        var node = new JsonObject { ["type"] = type };
        if (value is not null)
        {
            node["value"] = value;
        }
        return node;
    }

    private static string DescribeFault(ExecutionEngine engine)
    {
        // Neo.VM exposes no fault message on the public surface beyond the
        // state, so we classify generically. Both sides therefore compare
        // only the state and stack for FAULT vectors, plus this family tag.
        return "other";
    }

    /// <summary>
    /// Coarse fault classification, kept in lockstep with the Rust harness.
    ///
    /// The two implementations do not share exception type names, so we
    /// compare families rather than exact messages.
    /// </summary>
    private static string Classify(string message)
    {
        var m = message.ToLowerInvariant();
        if (m.Contains("overflow")) return "overflow";
        if (m.Contains("underflow") || m.Contains("stack is empty") || m.Contains("empty stack")
            || m.Contains("insufficient") || m.Contains("pop")) return "underflow";
        if (m.Contains("divide") || m.Contains("division") || m.Contains("modulo by zero"))
            return "divzero";
        if (m.Contains("index") || m.Contains("out of bounds") || m.Contains("range")) return "range";
        if (m.Contains("type") || m.Contains("cast") || m.Contains("expected") || m.Contains("not a"))
            return "type";
        if (m.Contains("unexpected") || m.Contains("invalid") || m.Contains("bad")) return "invalid";
        return "other";
    }
}
