mod config;
mod rpc;
mod scheduler;

use anyhow::Result;
use clap::Parser;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "regime_keeper", about = "Halcyon IL Protection regime keeper")]
struct Args {
    #[arg(long, default_value = "config/regime_keeper.json")]
    config: String,

    #[arg(long)]
    once: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();
    let cfg = config::KeeperConfig::load(&args.config)?;
    let client = rpc::KeeperClient::connect(&cfg).await?;

    info!(
        target = "regime_keeper",
        endpoint = %cfg.rpc_endpoint,
        product = %halcyon_il_protection::ID,
        "regime keeper starting",
    );

    if args.once {
        scheduler::run_once(&client, &cfg).await?;
        return Ok(());
    }

    let shutdown = tokio::signal::ctrl_c();
    tokio::select! {
        result = scheduler::run_forever(&client, &cfg) => {
            warn!(target = "regime_keeper", ?result, "scheduler exited");
            result
        }
        _ = shutdown => {
            info!(target = "regime_keeper", "SIGINT received; shutting down");
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
