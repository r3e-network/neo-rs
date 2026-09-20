// Cross-implementation P2P wire-format differential runner (C# side).
//
// Reads the vector file produced by neo-p2p/examples/p2p_wire_diff_runner.rs.
// For every vector it decodes the Rust bytes with Neo 3.10.1, re-encodes, and
// compares byte-for-byte. It also dumps the Neo 3.10.1 MessageCommand enum
// table so the Rust command bytes can be cross-checked by name.
//
// Usage:
//   dotnet run --project tools/csharp-p2p-runner -- <in_vectors.json> <out_csharp.json>

using System.Text.Json;
using System.Text.Json.Nodes;
using Neo.Extensions;
using Neo.IO;
using Neo.IO.Caching;
using Neo.Network.P2P;
using Neo.Network.P2P.Payloads;

namespace CSharpP2PRunner;

internal static class Program
{
    private static byte[] HexToBytes(string hex)
    {
        var output = new byte[hex.Length / 2];
        for (var i = 0; i < output.Length; i++)
            output[i] = Convert.ToByte(hex.Substring(i * 2, 2), 16);
        return output;
    }

    private static string BytesToHex(byte[] bytes) => Convert.ToHexStringLower(bytes);

    private static JsonObject EncodeResult(string name, byte[] reencoded) => new()
    {
        ["name"] = name,
        ["bytes"] = BytesToHex(reencoded),
    };

    private static int Main(string[] args)
    {
        if (args.Length != 2)
        {
            Console.Error.WriteLine("usage: csharp-p2p-runner <in.json> <out.json>");
            return 2;
        }

        using var doc = JsonDocument.Parse(File.ReadAllText(args[0]));
        var vectors = doc.RootElement.GetProperty("vectors");

        var results = new JsonArray();
        int pass = 0, fail = 0;

        foreach (var vector in vectors.EnumerateArray())
        {
            string name = vector.GetProperty("name").GetString()!;
            string kind = vector.GetProperty("kind").GetString()!;
            string expectedHex = vector.GetProperty("bytes").GetString()!;
            string? actualHex = null;
            string? error = null;

            try
            {
                switch (kind)
                {
                    case "version":
                    {
                        var reader = new MemoryReader(HexToBytes(expectedHex));
                        var payload = reader.ReadSerializable<VersionPayload>();
                        actualHex = BytesToHex(payload.ToArray());
                        break;
                    }
                    case "ping":
                    {
                        var reader = new MemoryReader(HexToBytes(expectedHex));
                        var payload = reader.ReadSerializable<PingPayload>();
                        actualHex = BytesToHex(payload.ToArray());
                        break;
                    }
                    case "inv":
                    {
                        var reader = new MemoryReader(HexToBytes(expectedHex));
                        var payload = reader.ReadSerializable<InvPayload>();
                        actualHex = BytesToHex(payload.ToArray());
                        break;
                    }
                    case "getblocks":
                    {
                        var reader = new MemoryReader(HexToBytes(expectedHex));
                        var payload = reader.ReadSerializable<GetBlocksPayload>();
                        actualHex = BytesToHex(payload.ToArray());
                        break;
                    }
                    case "getblockbyindex":
                    {
                        var reader = new MemoryReader(HexToBytes(expectedHex));
                        var payload = reader.ReadSerializable<GetBlockByIndexPayload>();
                        actualHex = BytesToHex(payload.ToArray());
                        break;
                    }
                    case "addr":
                    {
                        var reader = new MemoryReader(HexToBytes(expectedHex));
                        var payload = reader.ReadSerializable<AddrPayload>();
                        actualHex = BytesToHex(payload.ToArray());
                        break;
                    }
                    case "filterload":
                    {
                        var reader = new MemoryReader(HexToBytes(expectedHex));
                        var payload = reader.ReadSerializable<FilterLoadPayload>();
                        actualHex = BytesToHex(payload.ToArray());
                        break;
                    }
                    case "filteradd":
                    {
                        var reader = new MemoryReader(HexToBytes(expectedHex));
                        var payload = reader.ReadSerializable<FilterAddPayload>();
                        actualHex = BytesToHex(payload.ToArray());
                        break;
                    }
                    case "message":
                    {
                        // Decode the full wire frame with the real Message type.
                        var reader = new MemoryReader(HexToBytes(expectedHex));
                        var message = reader.ReadSerializable<Message>();
                        // Re-serialize uncompressed; the vectors use small payloads.
                        actualHex = BytesToHex(message.ToArray());
                        break;
                    }
                    default:
                        error = $"unknown kind {kind}";
                        break;
                }

                if (error is null)
                {
                    var ok = string.Equals(actualHex, expectedHex, StringComparison.Ordinal);
                    if (ok) pass++; else fail++;
                    results.Add(new JsonObject
                    {
                        ["name"] = name,
                        ["kind"] = kind,
                        ["expected"] = expectedHex,
                        ["actual"] = actualHex,
                        ["match"] = ok,
                    });
                }
                else
                {
                    fail++;
                    results.Add(new JsonObject
                    {
                        ["name"] = name,
                        ["kind"] = kind,
                        ["expected"] = expectedHex,
                        ["actual"] = actualHex,
                        ["match"] = false,
                        ["error"] = error,
                    });
                }
            }
            catch (Exception ex)
            {
                fail++;
                results.Add(new JsonObject
                {
                    ["name"] = name,
                    ["kind"] = kind,
                    ["expected"] = expectedHex,
                    ["actual"] = actualHex,
                    ["match"] = false,
                    ["error"] = ex.Message,
                });
            }
        }

        // Dump the authoritative Neo 3.10.1 MessageCommand byte table.
        var commandTable = new JsonObject();
        foreach (MessageCommand command in Enum.GetValues(typeof(MessageCommand)))
        {
            commandTable[command.ToString()] = (int)command;
        }

        var output = new JsonObject
        {
            ["total"] = pass + fail,
            ["matched"] = pass,
            ["mismatched"] = fail,
            ["results"] = results,
            ["command_table"] = commandTable,
        };
        File.WriteAllText(args[1], output.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
        Console.WriteLine($"csharp-p2p-runner: {pass}/{pass + fail} vectors matched");
        return fail == 0 ? 0 : 1;
    }
}
