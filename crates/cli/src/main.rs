//! Neo-Rust CLI - Production Node Interface
//!
//! This is the main CLI entry point for the Neo-Rust blockchain node.
//! It provides complete node functionality matching the C# Neo CLI exactly.

use anyhow::Result;
use clap::Parser;
use std::env;
use std::process;
use tracing::{debug, error, info, Level};
use tracing_subscriber::filter::EnvFilter;

mod args;
mod config;
mod console;
mod node;
mod rpc;
mod service;
mod wallet;

use args::CliArgs;
use service::MainService;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize enhanced logging with detailed formatting
    let env_filter = EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new("info"))?;

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_ansi(true)
        .init();

    // Show startup banner with detailed information
    show_startup_banner();

    // Parse command line arguments using clap Parser
    let args = CliArgs::parse();

    // Handle version flag
    if args.show_version {
        println!("neo-cli {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    info!("🚀 Starting Neo-Rust CLI");
    debug!("Command line arguments: {:?}", args);

    // Create and run main service
    let mut main_service = MainService::new(args).await?;

    // Run the main service
    match main_service.start().await {
        Ok(_) => {
            info!("✅ Neo CLI completed successfully");
            Ok(())
        }
        Err(e) => {
            error!("❌ Neo CLI failed: {}", e);
            process::exit(1);
        }
    }
}

/// Shows the startup banner with system information
fn show_startup_banner() {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║                    🔗 NEO RUST NODE v0.1.0 🔗                   ║");
    println!("║                                                                  ║");
    println!("║              Production-Ready Neo N3 Implementation             ║");
    println!("║                 Compatible with C# Neo Reference                ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();

    // System information
    info!("🖥️  System Information:");
    info!("   OS: {}", std::env::consts::OS);
    info!("   Architecture: {}", std::env::consts::ARCH);
    info!(
        "   Rust Version: {}",
        std::env!("CARGO_PKG_RUST_VERSION", "Unknown")
    );
    info!(
        "   Build Profile: {}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );

    // Performance information
    info!("⚡ Performance Capabilities:");
    info!("   🏎️  Async I/O with Tokio runtime");
    info!("   🧵 Multi-threaded processing");
    info!("   💾 Optimized memory management");
    info!("   🔄 Fast blockchain synchronization");

    println!();
}
