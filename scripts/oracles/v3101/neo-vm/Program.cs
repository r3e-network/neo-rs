using Neo.VM;
using Neo.VM.Types;
using System.Text.Json;

sealed class RecordingEngine : ExecutionEngine
{
    public Exception? FaultException { get; private set; }

    protected override void OnFault(Exception ex)
    {
        FaultException = ex;
        base.OnFault(ex);
    }
}

static class Oracle
{
    private static readonly List<Dictionary<string, object?>> Cases = [];

    private static Dictionary<string, object?> Result(RecordingEngine engine) => new()
    {
        ["state"] = engine.State.ToString(),
        ["invocation_stack_depth"] = engine.InvocationStack.Count,
        ["result_stack_depth"] = engine.ResultStack.Count,
        ["result_stack"] = Enumerable.Range(0, engine.ResultStack.Count)
            .Select(i => StackValue(engine.ResultStack.Peek(i))).ToArray(),
        ["fault_type"] = engine.FaultException?.GetType().Name,
        ["fault_message"] = engine.FaultException?.Message,
    };

    private static Dictionary<string, object?> StackValue(StackItem item) => new()
    {
        ["type"] = item.Type.ToString(),
        ["value"] = item switch
        {
            Neo.VM.Types.Integer value => value.GetInteger().ToString(),
            Neo.VM.Types.Boolean value => value.GetBoolean(),
            Null => null,
            _ => item.ToString(),
        },
    };

    private static void Add(string id, string operation, Dictionary<string, object?> observed) =>
        Cases.Add(new()
        {
            ["id"] = id,
            ["operation"] = operation,
            ["observed"] = observed,
        });

    private static Dictionary<string, object?> Capture(Action action)
    {
        try
        {
            action();
            return new() { ["outcome"] = "ok" };
        }
        catch (Exception ex)
        {
            return new()
            {
                ["outcome"] = "error",
                ["error_type"] = ex.GetType().Name,
                ["error_message"] = ex.Message,
            };
        }
    }

    private static RecordingEngine Execute(byte[] script, int rvcount = -1, bool strict = false)
    {
        var engine = new RecordingEngine();
        engine.LoadScript(new Script(script, strict), rvcount);
        engine.Execute();
        return engine;
    }

    public static void Main()
    {
        Add("implicit_ret_exact", "execute", Result(Execute([(byte)OpCode.PUSH1], 1)));
        Add("implicit_ret_too_few", "execute", Result(Execute([], 1)));
        Add("implicit_ret_too_many", "execute", Result(Execute([(byte)OpCode.PUSH1], 0)));

        Add("relaxed_unreachable_malformed", "execute", Result(Execute([(byte)OpCode.RET, 0xff])));
        Add("strict_unreachable_malformed", "script_construct", Capture(() =>
            new Script(new byte[] { (byte)OpCode.RET, 0xff }, true)));
        Add("strict_jump_to_end", "script_construct", Capture(() =>
            new Script(new byte[] { (byte)OpCode.JMP, 2 }, true)));
        Add("strict_convert_any", "script_construct", Capture(() =>
            new Script(new byte[] { (byte)OpCode.CONVERT, (byte)StackItemType.Any }, true)));

        var contextEngine = new RecordingEngine();
        var contextScript = new Script(new byte[] { (byte)OpCode.RET }, false);
        Add("context_at_script_end", "load_script", Capture(() =>
            contextEngine.LoadScript(contextScript, 0, contextScript.Length)));
        Add("context_beyond_script_end", "load_script", Capture(() =>
            new RecordingEngine().LoadScript(contextScript, 0, contextScript.Length + 1)));

        var call = new RecordingEngine();
        call.LoadScript(contextScript, 0);
        call.JumpTable.ExecuteCall(call, contextScript.Length);
        call.Execute();
        Add("call_to_script_end", "execute_call", Result(call));

        var jump = new RecordingEngine();
        jump.LoadScript(contextScript);
        Add("jump_to_script_end", "execute_jump", Capture(() =>
            jump.JumpTable.ExecuteJump(jump, contextScript.Length)));

        var invalidCatch = new RecordingEngine();
        invalidCatch.LoadScript(contextScript);
        invalidCatch.JumpTable.ExecuteTry(invalidCatch, 2, 0);
        var catchObserved = Capture(() =>
            invalidCatch.JumpTable.ExecuteThrow(invalidCatch, StackItem.Null));
        catchObserved["invocation_stack_depth"] = invalidCatch.InvocationStack.Count;
        Add("try_target_beyond_script_end", "execute_throw", catchObserved);

        var invalidEnd = new RecordingEngine();
        invalidEnd.LoadScript(contextScript);
        invalidEnd.JumpTable.ExecuteTry(invalidEnd, 1, 0);
        var endObserved = Capture(() => invalidEnd.JumpTable.ExecuteEndTry(invalidEnd, 2));
        endObserved["invocation_stack_depth"] = invalidEnd.InvocationStack.Count;
        Add("endtry_target_beyond_script_end", "execute_endtry", endObserved);

        foreach (var type in new[]
        {
            StackItemType.Map,
            StackItemType.Pointer,
            StackItemType.InteropInterface,
        })
        {
            var converted = StackItem.Null.ConvertTo(type);
            Add($"null_convert_{type.ToString().ToLowerInvariant()}", "convert_null", new()
            {
                ["requested_type"] = type.ToString(),
                ["result_type"] = converted.Type.ToString(),
                ["is_null"] = converted.IsNull,
            });
        }

        var slot = Execute([(byte)OpCode.PUSH1, (byte)OpCode.STLOC0]);
        var slotObserved = Result(slot);
        slotObserved["evaluation_stack_depth"] = slot.CurrentContext?.EvaluationStack.Count;
        Add("invalid_slot_store_preserves_operand", "execute", slotObserved);

        var staticIndex = Execute([
            (byte)OpCode.INITSSLOT,
            1,
            (byte)OpCode.PUSH1,
            (byte)OpCode.STSFLD,
            1,
        ]);
        var staticIndexObserved = Result(staticIndex);
        staticIndexObserved["evaluation_stack_depth"] =
            staticIndex.CurrentContext?.EvaluationStack.Count;
        Add("invalid_static_index_preserves_operand", "execute", staticIndexObserved);

        var localIndex = Execute([
            (byte)OpCode.INITSLOT,
            1,
            0,
            (byte)OpCode.PUSH1,
            (byte)OpCode.STLOC,
            1,
        ]);
        var localIndexObserved = Result(localIndex);
        localIndexObserved["evaluation_stack_depth"] =
            localIndex.CurrentContext?.EvaluationStack.Count;
        Add("invalid_local_index_preserves_operand", "execute", localIndexObserved);

        var argumentIndex = Execute([
            (byte)OpCode.PUSH1,
            (byte)OpCode.INITSLOT,
            0,
            1,
            (byte)OpCode.PUSH2,
            (byte)OpCode.STARG,
            1,
        ]);
        var argumentIndexObserved = Result(argumentIndex);
        argumentIndexObserved["evaluation_stack_depth"] =
            argumentIndex.CurrentContext?.EvaluationStack.Count;
        Add("invalid_argument_index_preserves_operand", "execute", argumentIndexObserved);

        var throwing = new RecordingEngine();
        throwing.LoadScript(contextScript, 0);
        throwing.LoadScript(new Script(
            new byte[] { (byte)OpCode.PUSH1, (byte)OpCode.THROW },
            false));
        throwing.Execute();
        var throwObserved = Result(throwing);
        throwObserved["current_evaluation_stack_depth"] =
            throwing.CurrentContext?.EvaluationStack.Count;
        Add("unhandled_throw_preserves_frames", "execute", throwObserved);

        using (var validAbort = new ScriptBuilder())
        {
            validAbort.EmitPush("NEO").Emit(OpCode.ABORTMSG);
            Add("abortmsg_valid_utf8", "execute", Result(Execute(validAbort.ToArray())));
        }
        using (var invalidAbort = new ScriptBuilder())
        {
            invalidAbort.EmitPush(new byte[] { 0xff }).Emit(OpCode.ABORTMSG);
            Add("abortmsg_invalid_utf8", "execute", Result(Execute(invalidAbort.ToArray())));
        }

        var output = new Dictionary<string, object?>
        {
            ["schema"] = 1,
            ["oracle"] = new Dictionary<string, object?>
            {
                ["repository"] = "https://github.com/neo-project/neo-vm.git",
                ["commit"] = "004cd6070a940405818d9357638277dd44407e2e",
                ["version"] = "3.10.1",
            },
            ["cases"] = Cases,
        };
        Console.WriteLine(JsonSerializer.Serialize(
            output,
            new JsonSerializerOptions { WriteIndented = true }));
    }
}
