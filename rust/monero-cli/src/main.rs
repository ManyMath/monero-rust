mod wallet_file;

use clap::{Parser, Subcommand};
use wallet_file::{resolve_wallet_path, WalletData};

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

        /// Save to an encrypted wallet file (will prompt for password)
        #[arg(long)]
        save: bool,

        /// Path to wallet file
        #[arg(long)]
        wallet: Option<String>,
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
        Command::Generate {
            seed_type,
            network,
            save,
            wallet,
        } => cmd_generate(&seed_type, &network, save, wallet.as_deref()),
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

fn cmd_generate(
    seed_type: &str,
    network: &str,
    save: bool,
    wallet_path: Option<&str>,
) -> Result<(), String> {
    let mnemonic = monero_rust::generate_seed(seed_type)?;
    let address = monero_rust::derive_address(&mnemonic, network)?;

    println!("=== New Monero Wallet ===");
    println!();
    println!("Seed phrase:");
    println!("  {}", mnemonic);
    println!();
    println!("Primary address:");
    println!("  {}", address);
    println!();
    println!("IMPORTANT: Write down your seed phrase and store it safely.");
    println!("Anyone with your seed phrase can access your funds.");

    if save {
        let password = rpassword::prompt_password("Set wallet password: ")
            .map_err(|e| format!("Failed to read password: {}", e))?;
        let confirm = rpassword::prompt_password("Confirm password: ")
            .map_err(|e| format!("Failed to read password: {}", e))?;
        if password != confirm {
            return Err("Passwords do not match".to_string());
        }

        let path = resolve_wallet_path(wallet_path)?;
        let data = WalletData {
            encrypted_seed: mnemonic,
            network: network.to_string(),
            last_sync_height: 0,
            outputs: Vec::new(),
        };
        wallet_file::save_wallet(&path, &data, &password)?;
        println!();
        println!("Wallet saved to: {}", path.display());
    }

    Ok(())
}

fn cmd_address(network: &str) -> Result<(), String> {
    let mnemonic = rpassword::prompt_password("Enter seed phrase: ")
        .map_err(|e| format!("Failed to read seed: {}", e))?;

    let mnemonic = mnemonic.trim();
    monero_rust::validate_seed(mnemonic)?;

    let keys = monero_rust::derive_keys(mnemonic, network)?;

    println!("=== Wallet Keys ===");
    println!();
    println!("Primary address:");
    println!("  {}", keys.address);
    println!();
    println!("Secret spend key:");
    println!("  {}", keys.secret_spend_key);
    println!();
    println!("Secret view key:");
    println!("  {}", keys.secret_view_key);
    println!();
    println!("Public spend key:");
    println!("  {}", keys.public_spend_key);
    println!();
    println!("Public view key:");
    println!("  {}", keys.public_view_key);

    Ok(())
}

fn cmd_balance(wallet_path: Option<&str>) -> Result<(), String> {
    let path = resolve_wallet_path(wallet_path)?;
    let password = rpassword::prompt_password("Wallet password: ")
        .map_err(|e| format!("Failed to read password: {}", e))?;

    let data = wallet_file::load_wallet(&path, &password)?;

    let total: u64 = data
        .outputs
        .iter()
        .filter(|o| !o.spent)
        .map(|o| o.amount)
        .sum();
    let spendable_count = data.outputs.iter().filter(|o| !o.spent).count();

    println!("=== Wallet Balance ===");
    println!();
    println!(
        "Balance: {:.12} XMR",
        total as f64 / 1_000_000_000_000.0
    );
    println!("Unspent outputs: {}", spendable_count);
    println!("Last synced height: {}", data.last_sync_height);

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

fn cmd_info(wallet_path: Option<&str>) -> Result<(), String> {
    let path = resolve_wallet_path(wallet_path)?;
    let password = rpassword::prompt_password("Wallet password: ")
        .map_err(|e| format!("Failed to read password: {}", e))?;

    let data = wallet_file::load_wallet(&path, &password)?;

    let address = monero_rust::derive_address(&data.encrypted_seed, &data.network)?;
    let keys = monero_rust::derive_keys(&data.encrypted_seed, &data.network)?;

    let total: u64 = data
        .outputs
        .iter()
        .filter(|o| !o.spent)
        .map(|o| o.amount)
        .sum();

    println!("=== Wallet Info ===");
    println!();
    println!("Wallet file: {}", path.display());
    println!("Network: {}", data.network);
    println!("Primary address: {}", address);
    println!("Secret view key: {}", keys.secret_view_key);
    println!("Last synced height: {}", data.last_sync_height);
    println!("Total outputs: {}", data.outputs.len());
    println!("Unspent outputs: {}", data.outputs.iter().filter(|o| !o.spent).count());
    println!(
        "Balance: {:.12} XMR",
        total as f64 / 1_000_000_000_000.0
    );

    Ok(())
}
