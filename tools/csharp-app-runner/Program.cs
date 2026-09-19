// Cross-implementation syscall differential execution runner (C# side).
//
// Mirrors neo-core/examples/vm_syscall_runner.rs: runs syscall scripts on a
// HOSTED Neo.SmartContract.ApplicationEngine and emits the same result schema
// as neo-vm/examples/vm_diff_runner.rs, so scripts/vm-diff.py can compare.
//
// Usage:
//   dotnet run --project tools/csharp-app-runner -- <in.json> <out.json>
//
// Pinned to Neo 3.10.1 (the audit baseline).
//
// Environment (must match the Rust runner exactly):
//   - ProtocolSettings.Default              (Network=0, AddressVersion=53)
//   - TriggerType.Application
//   - a minimal Transaction container       (crypto FALSE/FAULT paths only)
//   - a persisting Block, header Timestamp = PinnedBlockTimestamp
//
// Unlike the Rust ApplicationEngine, Neo's ApplicationEngine.Create() reads
// native-contract storage during construction (Ledger.CurrentIndex,
// Policy.GetExecPicoFeeFactor / GetStoragePrice) and throws KeyNotFound on an
// empty snapshot. We therefore seed the minimal genesis storage the
// constructor touches, hard-coded to the same default values neo-rs uses
// (Ledger index 0, Policy exec-fee factor 30, storage price 100000).

using System.Numerics;
using System.Text.Json;
using System.Text.Json.Nodes;
using Neo;
using Neo.Network.P2P.Payloads;
using Neo.Persistence;
using Neo.Persistence.Providers;
using Neo.SmartContract;
using Neo.SmartContract.Native;
using Neo.VM;
using Neo.VM.Types;

namespace CSharpAppRunner;

internal static class Program
{
    private const int RpcStackBudget = 2 * 1024 * 1024;
    private const ulong PinnedBlockTimestamp = 1_700_000_000;

    private static int Main(string[] args)
    {
        if (args.Length != 2)
        {
            Console.Error.WriteLine("usage: csharp-app-runner <in.json> <out.json>");
            return 2;
        }

        var raw = File.ReadAllText(args[0]);
        using var doc = JsonDocument.Parse(raw);
        var vectors = doc.RootElement.GetProperty("vectors");

        var results = new JsonArray();
        foreach (var vector in vectors.EnumerateArray())
            results.Add(RunOne(vector));

        var output = new JsonObject
        {
            ["impl_name"] = "csharp-neo-3.10.1-application-engine",
            ["results"] = results,
        };
        File.WriteAllText(args[1], output.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
        return 0;
    }

    private static JsonObject RunOne(JsonElement vector)
    {
        var name = vector.GetProperty("name").GetString() ?? "";
        var scriptHex = vector.GetProperty("script").GetString() ?? "";

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

        ApplicationEngine? engine;
        try
        {
            engine = BuildEngine();
        }
        catch (Exception ex)
        {
            return new JsonObject
            {
                ["name"] = name,
                ["state"] = "HARNESS_ERROR",
                ["stack"] = new JsonArray(),
                ["fault"] = null,
                ["harness_error"] = $"engine build failed: {ex.GetType().Name}: {ex.Message}",
            };
        }

        string? fault = null;
        try
        {
            engine.LoadScript(new Script(code), -1, 0);
            engine.Execute();
            if (engine.State == VMState.FAULT)
                fault = FaultOf(engine);
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
                stack.Add(RenderItem(item, ref budget));
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

    private static string? FaultOf(ApplicationEngine engine)
    {
        // The recorded fault is often a reflection wrapper; unwrap to the real cause.
        var ex = engine.FaultException;
        while (ex != null && (ex.Message == "Exception has been thrown by the target of an invocation."
                              || string.IsNullOrWhiteSpace(ex.Message)))
        {
            ex = ex.InnerException;
        }
        var msg = ex?.Message;
        if (!string.IsNullOrEmpty(msg)) return Classify(msg);
        return "other";
    }

    private static ApplicationEngine BuildEngine()
    {
        var store = new MemoryStore();
        var snapshot = new StoreCache(store, false);

        // Seed Ledger current-block (HashIndexState): { hash, index } struct.
        var ledgerKey = StorageKey.Create(-4, (byte)12); // LedgerContract native id
        var hashIndexState = new Neo.VM.Types.Struct(new Neo.VM.Types.StackItem[]
        {
            new Neo.VM.Types.ByteString(new byte[32]),
            new Neo.VM.Types.Integer(0),
        });
        snapshot.Add(ledgerKey, new StorageItem(BinarySerializer.Serialize(hashIndexState, ExecutionEngineLimits.Default)));

        // Seed Policy values (exec-fee factor 30, storage price 100000).
        const int policyId = -7;
        snapshot.Add(StorageKey.Create(policyId, (byte)18), new StorageItem(new BigInteger(30)));
        snapshot.Add(StorageKey.Create(policyId, (byte)19), new StorageItem(new BigInteger(100000)));

        var tx = new Transaction
        {
            Script = new byte[] { 0x01 },
            Signers = System.Array.Empty<Signer>(),
            Witnesses = System.Array.Empty<Witness>(),
            Attributes = System.Array.Empty<TransactionAttribute>(),
        };

        var header = new Header
        {
            Index = 1,
            Timestamp = PinnedBlockTimestamp,
            PrevHash = UInt256.Zero,
            MerkleRoot = UInt256.Zero,
            NextConsensus = UInt160.Zero,
            Witness = new Witness(),
        };
        var block = new Block { Header = header, Transactions = System.Array.Empty<Transaction>() };

        return ApplicationEngine.Create(
            TriggerType.Application,
            tx,
            snapshot,
            block,
            ProtocolSettings.Default,
            20_000_000_000,
            null);
    }

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
            case Neo.VM.Types.Null:
                return Node("Any", null); // canonical RPC envelope: Null => type "Any"
            case Neo.VM.Types.Array arr:
            {
                var kind = item is Struct ? "Struct" : "Array";
                var items = new JsonArray();
                foreach (var child in arr) items.Add(RenderItem(child, ref budget));
                return Node(kind, items);
            }
            case Map map:
            {
                var entries = new JsonArray();
                foreach (var (k, v) in map)
                    entries.Add(new JsonObject { ["key"] = RenderItem(k, ref budget), ["value"] = RenderItem(v, ref budget) });
                return Node("Map", entries);
            }
            case Pointer:
                return Node("Pointer", JsonValue.Create("0"));
            default:
                return Node("Any", null); // InteropInterface => "Any"
        }
    }

    private static JsonObject Node(string type, JsonNode? value)
    {
        var node = new JsonObject { ["type"] = type };
        if (value is not null) node["value"] = value;
        return node;
    }

    private static string Classify(string message)
    {
        var m = message.ToLowerInvariant();
        if (m.Contains("overflow")) return "overflow";
        if (m.Contains("underflow") || m.Contains("stack is empty") || m.Contains("empty stack")
            || m.Contains("insufficient") || m.Contains("pop")) return "underflow";
        if (m.Contains("divide") || m.Contains("division") || m.Contains("modulo by zero")) return "divzero";
        if (m.Contains("index") || m.Contains("out of bounds") || m.Contains("range")) return "range";
        if (m.Contains("public key") || m.Contains("ecpoint") || m.Contains("pubkey")) return "invalid";
        if (m.Contains("type") || m.Contains("cast") || m.Contains("expected") || m.Contains("not a")) return "type";
        if (m.Contains("unexpected") || m.Contains("invalid") || m.Contains("bad")) return "invalid";
        return "other";
    }
}