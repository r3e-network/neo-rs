using Neo.Cryptography;
using Neo.Cryptography.ECC;
using System.Text.Json;
using System.Text.Json.Nodes;

internal static class Program
{
    static byte[] HexToBytes(string hex)
    {
        var o = new byte[hex.Length / 2];
        for (int i = 0; i < o.Length; i++) o[i] = Convert.ToByte(hex.Substring(i * 2, 2), 16);
        return o;
    }

    static int Main(string[] args)
    {
        if (args.Length != 2) { Console.Error.WriteLine("usage: csharp-signature-runner <rust_out.json> <out.json>"); return 2; }
        using var doc = JsonDocument.Parse(File.ReadAllText(args[0]));
        var results = doc.RootElement.GetProperty("results");
        int total = 0, agree = 0, both = 0;
        var outArr = new JsonArray();
        foreach (var tx in results.EnumerateArray())
        {
            if (!tx.TryGetProperty("witnesses", out var wits)) continue;
            byte[] signData = HexToBytes(tx.GetProperty("sign_data").GetString()!);
            
            string txHash = tx.GetProperty("tx_hash").GetString()!;
            foreach (var wit in wits.EnumerateArray())
            {
                if (wit.GetProperty("kind").GetString() != "signature") continue;
                total++;
                byte[] pubkey = HexToBytes(wit.GetProperty("pubkey").GetString()!);
                byte[] sig = HexToBytes(wit.GetProperty("signature").GetString()!);
                bool rustVerify = wit.GetProperty("rust_verify").GetBoolean();
                bool csharpVerify = false;
                try
                {
                    var point = ECPoint.DecodePoint(pubkey, ECCurve.Secp256r1);
                    // Neo CheckSig semantic: verify SHA256(sign_data) signature,
                    // i.e. the message passed here is sign_data itself and the
                    // HashAlgorithm.SHA256 is applied inside VerifySignature.
                    csharpVerify = Crypto.VerifySignature(signData, sig, point, HashAlgorithm.SHA256);
                }
                catch { csharpVerify = false; }
                if (csharpVerify == rustVerify) agree++;
                if (rustVerify && csharpVerify) both++;
                outArr.Add(new JsonObject
                {
                    ["tx_hash"] = txHash,
                    ["pubkey"] = wit.GetProperty("pubkey").GetString(),
                    ["signature"] = wit.GetProperty("signature").GetString(),
                    ["rust_verify"] = rustVerify,
                    ["csharp_verify"] = csharpVerify,
                    ["agree"] = csharpVerify == rustVerify,
                });
            }
        }
        var output = new JsonObject
        {
            ["total_signature_witnesses"] = total,
            ["agree"] = agree,
            ["both_verify"] = both,
            ["results"] = outArr,
        };
        File.WriteAllText(args[1], output.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
        Console.WriteLine($"csharp-signature-runner: {agree}/{total} outcomes agree, {both} both-verify");
        return agree == total ? 0 : 1;
    }





}
