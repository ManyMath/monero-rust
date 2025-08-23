use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "monero-cli")]
#[command(about = "A pure-Rust Monero wallet CLI")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a new wallet seed phrase
    Generate {
        /// Seed type: "legacy" (25-word) or "polyseed" (16-word)
        #[arg(long, default_value = "legacy")]
        seed_type: String,

        /// Network: "mainnet", "stagenet", or "testnet"
        #[arg(long, default_value = "mainnet")]
        network: String,
    },

    /// Derive and display address/keys from a seed phrase
    Address {
        /// Network: "mainnet", "stagenet", or "testnet"
        #[arg(long, default_value = "mainnet")]
        network: String,
    },

    /// Show wallet balance
    Balance {
        /// Path to wallet file
        #[arg(long)]
        wallet: Option<String>,
    },

    /// Sync wallet with the blockchain
    Sync {
        /// Monero daemon RPC URL
        #[arg(long, default_value = "http://127.0.0.1:18081")]
        daemon: String,

        /// Path to wallet file
        #[arg(long)]
        wallet: Option<String>,
    },

    /// Transfer XMR to an address
    Transfer {
        /// Destination address
        address: String,

        /// Amount in XMR (e.g. "0.5")
        amount: String,

        /// Monero daemon RPC URL
        #[arg(long, default_value = "http://127.0.0.1:18081")]
        daemon: String,

        /// Path to wallet file
        #[arg(long)]
        wallet: Option<String>,
    },

    /// Display wallet info
    Info {
        /// Path to wallet file
        #[arg(long)]
        wallet: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Command::Generate { seed_type, network } => cmd_generate(&seed_type, &network),
        Command::Address { network } => cmd_address(&network),
        Command::Balance { wallet } => cmd_balance(wallet.as_deref()),
        Command::Sync { daemon, wallet } => cmd_sync(&daemon, wallet.as_deref()).await,
        Command::Transfer {
            address,
            amount,
            daemon,
            wallet,
        } => cmd_transfer(&address, &amount, &daemon, wallet.as_deref()),
        Command::Info { wallet } => cmd_info(wallet.as_deref()),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn cmd_generate(_seed_type: &str, _network: &str) -> Result<(), String> {
    println!("generate: not yet implemented");
    Ok(())
}

fn cmd_address(_network: &str) -> Result<(), String> {
    println!("address: not yet implemented");
    Ok(())
}

fn cmd_balance(_wallet: Option<&str>) -> Result<(), String> {
    println!("balance: not yet implemented");
    Ok(())
}

async fn cmd_sync(_daemon: &str, _wallet: Option<&str>) -> Result<(), String> {
    println!("sync: not yet implemented");
    Ok(())
}

fn cmd_transfer(
    _address: &str,
    _amount: &str,
    _daemon: &str,
    _wallet: Option<&str>,
) -> Result<(), String> {
    println!("transfer: not yet implemented");
    Ok(())
}

fn cmd_info(_wallet: Option<&str>) -> Result<(), String> {
    println!("info: not yet implemented");
    Ok(())
}
