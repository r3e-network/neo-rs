// Cross-implementation serialization differential runner (C# side, byte-level).
//
// Reads the vector file produced by neo-core/examples/serialization_diff_runner.rs.
// For every vector it:
//   - varint    : re-encodes <value> with Neo 3.10.1 WriteVarInt and compares bytes.
//   - varbytes  : re-encodes <payload> with Neo 3.10.1 WriteVarBytes and compares bytes.
//   - header    : decodes the Rust bytes with Neo 3.10.1 Header, re-encodes, compares.
//   - transaction: decodes with Neo 3.10.1 Transaction, re-encodes, compares.
//   - block     : decodes with Neo 3.10.1 Block, re-encodes, compares.
//
// The C# re-encoded bytes must byte-for-byte equal the Rust encoder's bytes for a
// PASS. A decode/re-encode roundtrip that differs, or that throws, is recorded as
// a mismatch (without masking it).
//
// Usage:
//   dotnet run --project tools/csharp-serialization-runner -- <in_vectors.json> <out_csharp.json>

using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Neo.Extensions;
using Neo.IO;
using Neo.Network.P2P.Payloads;

namespace CSharpSerializationRunner;

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

    private static int Main(string[] args)
    {
        if (args.Length != 2)
        {
            Console.Error.WriteLine("usage: csharp-serialization-runner <in.json> <out.json>");
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
            bool match = false;

            try
            {
                switch (kind)
                {
                    case "varint":
                    {
                        long value = vector.GetProperty("value").GetInt64();
                        using var ms = new MemoryStream();
                        using (var w = new BinaryWriter(ms))
                        {
                            w.WriteVarInt(value);
                        }
                        actualHex = BytesToHex(ms.ToArray());
                        break;
                    }
                    case "varbytes":
                    {
                        byte[] payload = HexToBytes(vector.GetProperty("payload").GetString()!);
                        using var ms = new MemoryStream();
                        using (var w = new BinaryWriter(ms))
                        {
                            w.WriteVarBytes(payload);
                        }
                        actualHex = BytesToHex(ms.ToArray());
                        break;
                    }
                    case "header":
                    {
                        byte[] bytes = HexToBytes(expectedHex);
                        var reader = new MemoryReader(bytes);
                        var header = reader.ReadSerializable<Header>();
                        using var ms = new MemoryStream();
                        using (var w = new BinaryWriter(ms))
                        {
                            header.Serialize(w);
                        }
                        actualHex = BytesToHex(ms.ToArray());
                        break;
                    }
                    case "transaction":
                    {
                        byte[] bytes = HexToBytes(expectedHex);
                        var reader = new MemoryReader(bytes);
                        var tx = reader.ReadSerializable<Transaction>();
                        using var ms = new MemoryStream();
                        using (var w = new BinaryWriter(ms))
                        {
                            ((ISerializable)tx).Serialize(w);
                        }
                        actualHex = BytesToHex(ms.ToArray());
                        break;
                    }
                    case "block":
                    {
                        byte[] bytes = HexToBytes(expectedHex);
                        var reader = new MemoryReader(bytes);
                        var block = reader.ReadSerializable<Block>();
                        using var ms = new MemoryStream();
                        using (var w = new BinaryWriter(ms))
                        {
                            ((ISerializable)block).Serialize(w);
                        }
                        actualHex = BytesToHex(ms.ToArray());
                        break;
                    }
                    default:
                        error = $"unknown kind {kind}";
                        break;
                }

                if (actualHex is not null)
                    match = expectedHex == actualHex;
            }
            catch (Exception ex)
            {
                error = $"{ex.GetType().Name}: {ex.Message}";
            }

            if (match) pass++; else fail++;

            var entry = new JsonObject
            {
                ["name"] = name,
                ["kind"] = kind,
                ["match"] = match,
            };
            if (actualHex is not null)
                entry["csharp_reencoded_hex"] = actualHex;
            if (error is not null)
                entry["error"] = error;
            results.Add(entry);
        }

        var output = new JsonObject
        {
            ["impl_name"] = "csharp-neo-3.10.1",
            ["total"] = vectors.GetArrayLength(),
            ["matched"] = pass,
            ["mismatched"] = fail,
            ["results"] = results,
        };
        File.WriteAllText(args[1], output.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));

        Console.WriteLine($"C# (Neo 3.10.1): total={vectors.GetArrayLength()} matched={pass} mismatched={fail}");
        return fail == 0 ? 0 : 1;
    }
}