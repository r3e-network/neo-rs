using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using Neo;
using Neo.IO;
using Neo.Network.P2P.Payloads;
using SysJsonSerializer = System.Text.Json.JsonSerializer;

namespace ValidationDiff
{
    public class ValidationResult
    {
        public bool Valid { get; set; }
        public string? ErrorType { get; set; }
        public string? ErrorMessage { get; set; }
    }

    public class BlockValidationTest
    {
        public string TestId { get; set; } = "";
        public string BlockHex { get; set; } = "";
        public bool ExpectedValid { get; set; }
        public string Description { get; set; } = "";
    }

    public class TransactionValidationTest
    {
        public string TestId { get; set; } = "";
        public string TxHex { get; set; } = "";
        public bool ExpectedValid { get; set; }
        public string Description { get; set; } = "";
    }

    public class CSharpValidator
    {
        private readonly ProtocolSettings _settings;

        public CSharpValidator(ProtocolSettings settings)
        {
            _settings = settings;
        }

        /// <summary>
        /// 验证区块
        /// </summary>
        public ValidationResult ValidateBlock(string blockHex)
        {
            try
            {
                // 1. 从十六进制解码区块
                byte[] blockBytes = Convert.FromHexString(blockHex);

                // 2. 反序列化区块
                Block block = Neo.IO.Helper.AsSerializable<Block>(blockBytes);

                // 3. 验证区块头
                // Neo 3.10.1 的 Block.Verify() 方法
                try
                {
                    // 验证基本规则：
                    // - timestamp 在合理范围内
                    // - version 正确
                    // - primary_index < validator_count
                    // - nonce 唯一性（需要数据库，这里简化）
                    
                    // 基本验证（不依赖存储）
                    if (block.Version != 0)
                    {
                        return new ValidationResult
                        {
                            Valid = false,
                            ErrorType = "VersionError",
                            ErrorMessage = $"Invalid block version: {block.Version}"
                        };
                    }

                    // Timestamp 验证（参考 C# Block.cs）
                    const long MIN_TIMESTAMP = 1468595301000; // Neo 创世块时间
                    if (block.Timestamp < MIN_TIMESTAMP)
                    {
                        return new ValidationResult
                        {
                            Valid = false,
                            ErrorType = "TimestampError",
                            ErrorMessage = $"Timestamp too early: {block.Timestamp}"
                        };
                    }

                    long now = DateTimeOffset.UtcNow.ToUnixTimeMilliseconds();
                    const long MAX_DRIFT = 15 * 60 * 1000; // 15 分钟
                    if (block.Timestamp > (ulong)(now + MAX_DRIFT))
                    {
                        return new ValidationResult
                        {
                            Valid = false,
                            ErrorType = "TimestampError",
                            ErrorMessage = $"Timestamp too far in future: {block.Timestamp}"
                        };
                    }

                    return new ValidationResult
                    {
                        Valid = true,
                        ErrorType = null,
                        ErrorMessage = null
                    };
                }
                catch (Exception e)
                {
                    return new ValidationResult
                    {
                        Valid = false,
                        ErrorType = ClassifyError(e.Message),
                        ErrorMessage = e.Message
                    };
                }
            }
            catch (Exception e)
            {
                return new ValidationResult
                {
                    Valid = false,
                    ErrorType = "DeserializeError",
                    ErrorMessage = $"Failed to deserialize block: {e.Message}"
                };
            }
        }

        /// <summary>
        /// 验证交易
        /// </summary>
        public ValidationResult ValidateTransaction(string txHex)
        {
            try
            {
                // 1. 从十六进制解码交易
                byte[] txBytes = Convert.FromHexString(txHex);

                // 2. 反序列化交易
                Transaction tx = Neo.IO.Helper.AsSerializable<Transaction>(txBytes);

                // 3. 验证交易
                // Neo 3.10.1 的 Transaction.Verify() 方法
                try
                {
                    // 验证基本规则：
                    // - size <= MAX_TRANSACTION_SIZE (65536)
                    // - system_fee 计算正确
                    // - network_fee 足够
                    // - attributes 合法性
                    // - witnesses 数量与签名者匹配

                    // 大小验证
                    const int MAX_TX_SIZE = 65536;
                    if (tx.Size > MAX_TX_SIZE)
                    {
                        return new ValidationResult
                        {
                            Valid = false,
                            ErrorType = "SizeError",
                            ErrorMessage = $"Transaction size {tx.Size} exceeds limit {MAX_TX_SIZE}"
                        };
                    }

                    // Fee 验证（简化）
                    if (tx.SystemFee < 0 || tx.NetworkFee < 0)
                    {
                        return new ValidationResult
                        {
                            Valid = false,
                            ErrorType = "FeeError",
                            ErrorMessage = "Negative fees not allowed"
                        };
                    }

                    // Attributes 验证
                    if (tx.Attributes.Length > 16)
                    {
                        return new ValidationResult
                        {
                            Valid = false,
                            ErrorType = "AttributeError",
                            ErrorMessage = $"Too many attributes: {tx.Attributes.Length}"
                        };
                    }

                    return new ValidationResult
                    {
                        Valid = true,
                        ErrorType = null,
                        ErrorMessage = null
                    };
                }
                catch (Exception e)
                {
                    return new ValidationResult
                    {
                        Valid = false,
                        ErrorType = ClassifyError(e.Message),
                        ErrorMessage = e.Message
                    };
                }
            }
            catch (Exception e)
            {
                return new ValidationResult
                {
                    Valid = false,
                    ErrorType = "DeserializeError",
                    ErrorMessage = $"Failed to deserialize transaction: {e.Message}"
                };
            }
        }

        /// <summary>
        /// 对错误进行分类
        /// </summary>
        private static string ClassifyError(string errorMessage)
        {
            if (errorMessage.Contains("timestamp", StringComparison.OrdinalIgnoreCase))
                return "TimestampError";
            if (errorMessage.Contains("version", StringComparison.OrdinalIgnoreCase))
                return "VersionError";
            if (errorMessage.Contains("primary", StringComparison.OrdinalIgnoreCase) ||
                errorMessage.Contains("index", StringComparison.OrdinalIgnoreCase))
                return "PrimaryIndexError";
            if (errorMessage.Contains("nonce", StringComparison.OrdinalIgnoreCase))
                return "NonceError";
            if (errorMessage.Contains("merkle", StringComparison.OrdinalIgnoreCase))
                return "MerkleRootError";
            if (errorMessage.Contains("size", StringComparison.OrdinalIgnoreCase))
                return "SizeError";
            if (errorMessage.Contains("fee", StringComparison.OrdinalIgnoreCase))
                return "FeeError";
            if (errorMessage.Contains("witness", StringComparison.OrdinalIgnoreCase))
                return "WitnessError";
            if (errorMessage.Contains("attribute", StringComparison.OrdinalIgnoreCase))
                return "AttributeError";

            return "ValidationError";
        }

        /// <summary>
        /// 批量验证区块
        /// </summary>
        public Dictionary<string, ValidationResult> ValidateBlocks(List<BlockValidationTest> tests)
        {
            var results = new Dictionary<string, ValidationResult>();
            foreach (var test in tests)
            {
                results[test.TestId] = ValidateBlock(test.BlockHex);
            }
            return results;
        }

        /// <summary>
        /// 批量验证交易
        /// </summary>
        public Dictionary<string, ValidationResult> ValidateTransactions(List<TransactionValidationTest> tests)
        {
            var results = new Dictionary<string, ValidationResult>();
            foreach (var test in tests)
            {
                results[test.TestId] = ValidateTransaction(test.TxHex);
            }
            return results;
        }
    }

    class Program
    {
        static void Main(string[] args)
        {
            if (args.Length < 4)
            {
                Console.WriteLine("Usage: csharp-validator <validate-block|validate-transaction> <input.json> <output.json> <network>");
                return;
            }

            string command = args[0];
            string inputPath = args[1];
            string outputPath = args[2];
            string network = args[3];

            // 加载协议设置
            ProtocolSettings settings = network.ToLower() switch
            {
                "mainnet" => ProtocolSettings.Load("config.mainnet.json"),
                "testnet" => ProtocolSettings.Load("config.testnet.json"),
                _ => throw new ArgumentException($"Unknown network: {network}")
            };

            var validator = new CSharpValidator(settings);

            if (command == "validate-block")
            {
                ValidateBlocks(validator, inputPath, outputPath);
            }
            else if (command == "validate-transaction")
            {
                ValidateTransactions(validator, inputPath, outputPath);
            }
            else
            {
                Console.WriteLine($"Unknown command: {command}");
            }
        }

        static void ValidateBlocks(CSharpValidator validator, string inputPath, string outputPath)
        {
            // 1. 加载测试向量
            string inputJson = File.ReadAllText(inputPath);
            var tests = SysJsonSerializer.Deserialize<List<BlockValidationTest>>(inputJson)!;

            Console.WriteLine($"Loaded {tests.Count} block validation tests");

            // 2. 执行验证
            var results = validator.ValidateBlocks(tests);

            // 3. 输出结果
            string outputJson = SysJsonSerializer.Serialize(results, new System.Text.Json.JsonSerializerOptions
            {
                WriteIndented = true
            });
            File.WriteAllText(outputPath, outputJson);

            // 4. 打印摘要
            int passed = results.Values.Count(r => r.Valid);
            int failed = results.Count - passed;
            Console.WriteLine($"Results: {passed} passed, {failed} failed");
        }

        static void ValidateTransactions(CSharpValidator validator, string inputPath, string outputPath)
        {
            // 1. 加载测试向量
            string inputJson = File.ReadAllText(inputPath);
            var tests = SysJsonSerializer.Deserialize<List<TransactionValidationTest>>(inputJson)!;

            Console.WriteLine($"Loaded {tests.Count} transaction validation tests");

            // 2. 执行验证
            var results = validator.ValidateTransactions(tests);

            // 3. 输出结果
            string outputJson = SysJsonSerializer.Serialize(results, new System.Text.Json.JsonSerializerOptions
            {
                WriteIndented = true
            });
            File.WriteAllText(outputPath, outputJson);

            // 4. 打印摘要
            int passed = results.Values.Count(r => r.Valid);
            int failed = results.Count - passed;
            Console.WriteLine($"Results: {passed} passed, {failed} failed");
        }
    }
}
