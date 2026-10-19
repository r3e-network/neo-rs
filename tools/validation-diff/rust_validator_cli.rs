//! Rust 验证器 CLI 工具

use anyhow::Result;
use clap::{Parser, Subcommand};
use neo_core::ProtocolSettings;
use std::fs;
use std::path::PathBuf;

mod rust_validator;
use rust_validator::{BlockValidationTest, RustValidator, TransactionValidationTest};

#[derive(Parser)]
#[command(name = "rust-validator")]
#[command(about = "Rust 侧区块/交易验证器", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 验证区块
    ValidateBlock {
        /// 测试向量 JSON 文件路径
        #[arg(short, long)]
        input: PathBuf,

        /// 输出结果 JSON 文件路径
        #[arg(short, long)]
        output: PathBuf,

        /// 网络（mainnet/testnet）
        #[arg(short, long, default_value = "mainnet")]
        network: String,
    },
    /// 验证交易
    ValidateTransaction {
        /// 测试向量 JSON 文件路径
        #[arg(short, long)]
        input: PathBuf,

        /// 输出结果 JSON 文件路径
        #[arg(short, long)]
        output: PathBuf,

        /// 网络（mainnet/testnet）
        #[arg(short, long, default_value = "mainnet")]
        network: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::ValidateBlock {
            input,
            output,
            network,
        } => validate_blocks(input, output, &network)?,
        Commands::ValidateTransaction {
            input,
            output,
            network,
        } => validate_transactions(input, output, &network)?,
    }

    Ok(())
}

fn validate_blocks(input: PathBuf, output: PathBuf, network: &str) -> Result<()> {
    // 1. 加载协议设置
    let protocol_settings = match network {
        "mainnet" => ProtocolSettings::mainnet(),
        "testnet" => ProtocolSettings::testnet(),
        _ => anyhow::bail!("Unknown network: {}", network),
    };

    // 2. 创建验证器
    let validator = RustValidator::new(protocol_settings);

    // 3. 加载测试向量
    let input_json = fs::read_to_string(input)?;
    let tests: Vec<BlockValidationTest> = serde_json::from_str(&input_json)?;

    println!("Loaded {} block validation tests", tests.len());

    // 4. 执行验证
    let results = validator.validate_blocks(&tests);

    // 5. 输出结果
    let output_json = serde_json::to_string_pretty(&results)?;
    fs::write(output, output_json)?;

    // 6. 打印摘要
    let passed = results.values().filter(|r| r.valid).count();
    let failed = results.len() - passed;
    println!("Results: {} passed, {} failed", passed, failed);

    Ok(())
}

fn validate_transactions(input: PathBuf, output: PathBuf, network: &str) -> Result<()> {
    // 1. 加载协议设置
    let protocol_settings = match network {
        "mainnet" => ProtocolSettings::mainnet(),
        "testnet" => ProtocolSettings::testnet(),
        _ => anyhow::bail!("Unknown network: {}", network),
    };

    // 2. 创建验证器
    let validator = RustValidator::new(protocol_settings);

    // 3. 加载测试向量
    let input_json = fs::read_to_string(input)?;
    let tests: Vec<TransactionValidationTest> = serde_json::from_str(&input_json)?;

    println!("Loaded {} transaction validation tests", tests.len());

    // 4. 执行验证
    let results = validator.validate_transactions(&tests);

    // 5. 输出结果
    let output_json = serde_json::to_string_pretty(&results)?;
    fs::write(output, output_json)?;

    // 6. 打印摘要
    let passed = results.values().filter(|r| r.valid).count();
    let failed = results.len() - passed;
    println!("Results: {} passed, {} failed", passed, failed);

    Ok(())
}
