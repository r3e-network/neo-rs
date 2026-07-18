using Neo;
using Neo.Extensions;
using Neo.Network.P2P.Payloads;
using Neo.Persistence;
using Neo.Persistence.Providers;
using Neo.SmartContract;
using Neo.VM;
using System.Collections.Immutable;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json;
using VMArray = Neo.VM.Types.Array;

static class Oracle
{
    private static readonly List<Dictionary<string, object?>> Cases = [];

    private static void Add(
        string id,
        string operation,
        string[] hardforks,
        Dictionary<string, object?> observed) => Cases.Add(new()
        {
            ["id"] = id,
            ["operation"] = operation,
            ["hardforks"] = hardforks,
            ["observed"] = observed,
        });

    private static ProtocolSettings Settings(
        params (Hardfork hardfork, uint height)[] activations) =>
        ProtocolSettings.Default with
        {
            Hardforks = activations.ToImmutableDictionary(
                pair => pair.hardfork,
                pair => pair.height),
        };

    private static ApplicationEngine Engine(DataCache snapshot, ProtocolSettings settings)
    {
        return ApplicationEngine.Create(
            TriggerType.Application,
            null,
            snapshot,
            new Block
            {
                Header = (Header)RuntimeHelpers.GetUninitializedObject(typeof(Header)),
                Transactions = [],
            },
            settings,
            ApplicationEngine.TestModeGas);
    }

    private static Dictionary<string, object?> Capture(Action action)
    {
        try
        {
            action();
            return new() { ["outcome"] = "ok" };
        }
        catch (TargetInvocationException ex) when (ex.InnerException is not null)
        {
            return Error(ex.InnerException);
        }
        catch (Exception ex)
        {
            return Error(ex);
        }
    }

    private static Dictionary<string, object?> Error(Exception ex) => new()
    {
        ["outcome"] = "error",
        ["error_type"] = ex.GetType().Name,
        ["error_message"] = ex.Message,
    };

    private static void RecordRuntimeLoadScript(
        DataCache snapshot,
        string id,
        string era,
        ProtocolSettings settings,
        byte[] script)
    {
        using var engine = Engine(snapshot, settings);
        engine.LoadScript(new byte[] { (byte)OpCode.RET });
        var method = typeof(ApplicationEngine).GetMethod(
            "RuntimeLoadScript",
            BindingFlags.Instance | BindingFlags.NonPublic)!;
        var observed = Capture(() => method.Invoke(
            engine,
            new object[] { script, CallFlags.ReadOnly, new VMArray() }));
        observed["invocation_stack_depth"] = engine.InvocationStack.Count;
        Add(id, "runtime_load_script", [era], observed);
    }

    private static void RecordJumpTable(
        DataCache snapshot,
        string id,
        string era,
        ProtocolSettings settings)
    {
        using var engine = Engine(snapshot, settings);
        var handlers = new Dictionary<string, object?>();
        foreach (var opcode in new[]
        {
            OpCode.SUBSTR,
            OpCode.HASKEY,
            OpCode.PICKITEM,
            OpCode.SETITEM,
            OpCode.REMOVE,
            OpCode.SHL,
            OpCode.SHR,
        })
        {
            handlers[opcode.ToString()] = engine.JumpTable[opcode].Method.Name;
        }
        Add(id, "jump_table", [era], new() { ["handlers"] = handlers });
    }

    public static void Main()
    {
        using var store = new MemoryStore();
        using var storeSnapshot = store.GetSnapshot();
        using var snapshot = new StoreCache(storeSnapshot);

        var preBasilisk = Settings(
            (Hardfork.HF_Basilisk, 1),
            (Hardfork.HF_Echidna, 2),
            (Hardfork.HF_Gorgon, 3));
        var postBasilisk = Settings(
            (Hardfork.HF_Basilisk, 0),
            (Hardfork.HF_Echidna, 2),
            (Hardfork.HF_Gorgon, 3));

        foreach (var (suffix, era, settings) in new[]
        {
            ("pre_basilisk", "pre_basilisk", preBasilisk),
            ("post_basilisk", "basilisk_and_later", postBasilisk),
        })
        {
            RecordRuntimeLoadScript(
                snapshot,
                $"runtime_load_script_invalid_jump_{suffix}",
                era,
                settings,
                new byte[] { (byte)OpCode.JMP, 0x7f });
            RecordRuntimeLoadScript(
                snapshot,
                $"runtime_load_script_convert_any_{suffix}",
                era,
                settings,
                new byte[] { (byte)OpCode.CONVERT, 0x00 });
        }

        using (var engine = Engine(snapshot, ProtocolSettings.Default))
        {
            engine.LoadScript(new byte[] { (byte)OpCode.ABORT });
            var send = typeof(ApplicationEngine).GetMethod(
                "SendNotification",
                BindingFlags.Instance | BindingFlags.NonPublic)!;
            send.Invoke(
                engine,
                new object[] { UInt160.Zero, "BeforeFault", new VMArray() });
            var before = engine.Notifications.Count;
            var state = engine.Execute();
            Add("fault_clears_notifications", "application_fault", ["all"], new()
            {
                ["state"] = state.ToString(),
                ["notifications_before"] = before,
                ["notifications_after"] = engine.Notifications.Count,
                ["fault_type"] = engine.FaultException?.GetType().Name,
                ["fault_message"] = engine.FaultException?.Message,
            });
        }

        using (var builder = new ScriptBuilder())
        {
            builder.CreateStruct(new[] { 1, 2 });
            Add("script_builder_struct_uses_packstruct", "script_builder", ["all"], new()
            {
                ["script_hex"] = Convert.ToHexString(builder.ToArray()).ToLowerInvariant(),
                ["last_opcode"] = ((OpCode)builder.ToArray()[^1]).ToString(),
            });
        }

        RecordJumpTable(
            snapshot,
            "jump_table_before_echidna",
            "pre_echidna",
            Settings((Hardfork.HF_Echidna, 1), (Hardfork.HF_Gorgon, 2)));
        RecordJumpTable(
            snapshot,
            "jump_table_echidna_before_gorgon",
            "echidna_to_gorgon",
            Settings((Hardfork.HF_Echidna, 0), (Hardfork.HF_Gorgon, 1)));
        RecordJumpTable(
            snapshot,
            "jump_table_gorgon_and_later",
            "gorgon_and_later",
            Settings((Hardfork.HF_Echidna, 0), (Hardfork.HF_Gorgon, 0)));

        var output = new Dictionary<string, object?>
        {
            ["schema"] = 1,
            ["oracle"] = new Dictionary<string, object?>
            {
                ["repository"] = "https://github.com/neo-project/neo.git",
                ["commit"] = "d10e9ceecdabe3fcff719ee68ea5b76ba7e62c3d",
                ["version"] = "3.10.1",
            },
            ["cases"] = Cases,
        };
        Console.WriteLine(System.Text.Json.JsonSerializer.Serialize(
            output,
            new JsonSerializerOptions { WriteIndented = true }));
    }
}
