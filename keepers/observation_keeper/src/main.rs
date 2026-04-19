mod config;
mod rpc;
mod scheduler;

use anyhow::Result;
use clap::Parser;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(
    name = "observation_keeper",
    about = "Halcyon SOL Autocall observation keeper"
)]
struct Args {
    #[arg(long, default_value = "config/observation_keeper.json")]
    config: String,

    #[arg(long)]
    once: bool,

    #[arg(long)]
    policy: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();
    let cfg = config::KeeperConfig::load(&args.config)?;
    let client = rpc::KeeperClient::connect(&cfg).await?;
    let policy_filter = args.policy.as_deref().map(Pubkey::from_str).transpose()?;

    info!(
        target = "observation_keeper",
        endpoint = %cfg.rpc_endpoint,
        product = %cfg.sol_autocall_program_id,
        "observation keeper starting",
    );

    if args.once {
        scheduler::run_once(&client, &cfg, policy_filter).await?;
        return Ok(());
    }

    let shutdown = tokio::signal::ctrl_c();
    tokio::select! {
        result = scheduler::run_forever(&client, &cfg) => {
            warn!(target = "observation_keeper", ?result, "scheduler exited");
            result
        }
        _ = shutdown => {
            info!(target = "observation_keeper", "SIGINT received; shutting down");
            Ok(())
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt()
        .with_env_filter(filter)
        .json()
        .with_current_span(false)
        .init();
}
