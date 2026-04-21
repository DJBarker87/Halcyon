//! Print the deterministic `PriceUpdateV2` account addresses the relay
//! would write to. Runnable without the Node TypeScript toolchain.
//!
//! Usage:
//!   cargo run --manifest-path keepers/price_relay/Cargo.toml --bin print_feed_addresses -- --shard 7
//!
//! Derivation matches `@pythnetwork/pyth-solana-receiver`:
//!   PDA = findProgramAddress([shard_le_u16, feed_id_32_bytes], push_oracle_program)

use anyhow::{Context, Result};
use clap::Parser;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

const PYTH_PUSH_ORACLE_ID: &str = "pythWSnswVUd12oZpeFP8e9CVaEqJg25g1Vtc2biRsT";

#[derive(Parser, Debug)]
#[command(name = "print_feed_addresses", about = "Halcyon price relay — print feed PDAs")]
struct Args {
    /// Shard ID (u16) — stable identifier for this relay instance's
    /// PriceUpdateV2 accounts. Must match `shard_id` in the relay config.
    #[arg(long, default_value_t = 7u16)]
    shard: u16,

    /// `devnet` or `mainnet` — controls the env var suffix in output.
    #[arg(long, default_value = "devnet")]
    cluster: String,
}

struct Feed {
    alias: &'static str,
    symbol: &'static str,
    feed_id: [u8; 32],
}

const FEEDS: &[Feed] = &[
    Feed {
        alias: "SOL_USD",
        symbol: "SOL",
        feed_id: hex!("ef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d"),
    },
    Feed {
        alias: "USDC_USD",
        symbol: "USDC",
        feed_id: hex!("eaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a"),
    },
    Feed {
        alias: "SPY_USD",
        symbol: "SPY",
        feed_id: hex!("19e09bb805456ada3979a7d1cbb4b6d63babc3a0f8e8a9509f68afa5c4c11cd5"),
    },
    Feed {
        alias: "QQQ_USD",
        symbol: "QQQ",
        feed_id: hex!("9695e2b96ea7b3859da9ed25b7a46a920a776e2fdae19a7bcfdf2b219230452d"),
    },
    Feed {
        alias: "IWM_USD",
        symbol: "IWM",
        feed_id: hex!("eff690a187797aa225723345d4612abec0bf0cec1ae62347c0e7b1905d730879"),
    },
];

/// Compile-time hex → [u8; 32]. Pulled in rather than depending on the
/// `hex-literal` crate so this binary has near-zero transitive deps.
#[macro_export]
macro_rules! hex {
    ($s:literal) => {{
        const BYTES: [u8; 32] = const_hex_decode($s);
        BYTES
    }};
}

const fn const_hex_decode(input: &str) -> [u8; 32] {
    let bytes = input.as_bytes();
    assert!(bytes.len() == 64, "feed id hex must be 64 chars");
    let mut out = [0u8; 32];
    let mut i = 0usize;
    while i < 32 {
        let hi = hex_char_to_nibble(bytes[i * 2]);
        let lo = hex_char_to_nibble(bytes[i * 2 + 1]);
        out[i] = (hi << 4) | lo;
        i += 1;
    }
    out
}

const fn hex_char_to_nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => panic!("invalid hex char"),
    }
}

fn feed_account_address(shard: u16, feed_id: &[u8; 32], program: &Pubkey) -> Pubkey {
    let shard_le = shard.to_le_bytes();
    let (pda, _bump) = Pubkey::find_program_address(&[&shard_le, feed_id], program);
    pda
}

fn main() -> Result<()> {
    let args = Args::parse();
    let program = Pubkey::from_str(PYTH_PUSH_ORACLE_ID)
        .context("hard-coded Pyth push oracle program ID is invalid")?;

    let env_suffix = match args.cluster.as_str() {
        "mainnet" => "MAINNET",
        "devnet" => "DEVNET",
        "localnet" => "LOCALNET",
        other => panic!("unknown cluster: {other}"),
    };

    println!("shard_id     = {}", args.shard);
    println!("push_oracle  = {}", PYTH_PUSH_ORACLE_ID);
    println!("cluster      = {}\n", args.cluster);

    for feed in FEEDS {
        let address = feed_account_address(args.shard, &feed.feed_id, &program);
        let env_key = format!("NEXT_PUBLIC_PYTH_{}_ACCOUNT_{env_suffix}", feed.symbol);
        println!("{}", feed.alias);
        println!("  account : {address}");
        println!("  env     : {env_key}={address}\n");
    }

    Ok(())
}
