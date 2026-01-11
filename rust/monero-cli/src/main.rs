mod wallet_file;

use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use wallet_file::{resolve_wallet_path, WalletData};

fn parse_xmr_to_piconero(s: &str) -> Result<u64, String> {
    let s = s.trim();
    let parts: Vec<&str> = s.split('.').collect();
    match parts.len() {
        1 => {
            let whole: u64 = parts[0]
                .parse()
                .map_err(|_| format!("Invalid amount: {}", s))?;
            whole
                .checked_mul(1_000_000_000_000)
                .ok_or_else(|| "Amount overflow".to_string())
        }
        2 => {
            let whole: u64 = parts[0]
                .parse()
                .map_err(|_| format!("Invalid amount: {}", s))?;
            let frac_str = parts[1];
            if frac_str.len() > 12 {
                return Err("Too many decimal places (max 12)".to_string());
            }
            let padded = format!("{:0<12}", frac_str);
            let frac: u64 = padded
                .parse()
                .map_err(|_| format!("Invalid amount: {}", s))?;
            whole
                .checked_mul(1_000_000_000_000)
                .and_then(|w| w.checked_add(frac))
                .ok_or_else(|| "Amount overflow".to_string())
        }
        _ => Err(format!("Invalid amount format: {}", s)),
    }
}

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

    /// Sweep all funds to an address (sends entire balance minus fee)
    SweepAll {
        /// Destination address
        address: String,

        /// Monero daemon RPC URL
        #[arg(long, default_value = "http://127.0.0.1:18081")]
        daemon: String,

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
        } => cmd_transfer(&address, &amount, &daemon, wallet.as_deref()).await,
        Command::Info { wallet } => cmd_info(wallet.as_deref()),
        Command::SweepAll {
            address,
            daemon,
            wallet,
        } => cmd_sweep_all(&address, &daemon, wallet.as_deref()).await,
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
    let address = monero_rust::derive_address(&mnemonic, network, "")?;

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
        if password.is_empty() {
            eprint!("Warning: empty password provides no protection. Continue? [y/N]: ");
            let mut confirm_empty = String::new();
            std::io::stdin()
                .read_line(&mut confirm_empty)
                .map_err(|e| format!("Failed to read input: {}", e))?;
            if !confirm_empty.trim().eq_ignore_ascii_case("y") {
                return Err("Aborted".to_string());
            }
        } else {
            let confirm = rpassword::prompt_password("Confirm password: ")
                .map_err(|e| format!("Failed to read password: {}", e))?;
            if password != confirm {
                return Err("Passwords do not match".to_string());
            }
        }

        let path = resolve_wallet_path(wallet_path)?;
        let data = WalletData {
            mnemonic: mnemonic,
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

    let keys = monero_rust::derive_keys(mnemonic, network, "")?;

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

    let unspent: Vec<&monero_rust::WalletOutput> =
        data.outputs.iter().filter(|o| !o.spent).collect();

    let sync_height = data.last_sync_height;

    let mut unlocked_total: u64 = 0;
    let mut locked_total: u64 = 0;
    let mut unlocked_count: usize = 0;
    let mut locked_count: usize = 0;

    for o in &unspent {
        if monero_rust::is_spendable(o, sync_height) {
            unlocked_total += o.amount;
            unlocked_count += 1;
        } else {
            locked_total += o.amount;
            locked_count += 1;
        }
    }

    let total = unlocked_total + locked_total;

    println!("=== Wallet Balance ===");
    println!();
    println!("Total:    {:.12} XMR", total as f64 / 1_000_000_000_000.0);
    println!(
        "Unlocked: {:.12} XMR  ({} outputs)",
        unlocked_total as f64 / 1_000_000_000_000.0,
        unlocked_count
    );
    println!(
        "Locked:   {:.12} XMR  ({} outputs)",
        locked_total as f64 / 1_000_000_000_000.0,
        locked_count
    );
    println!();
    println!("Total unspent outputs: {}", unspent.len());
    println!("Last synced height: {}", sync_height);

    Ok(())
}

async fn cmd_sync(daemon: &str, wallet_path: Option<&str>) -> Result<(), String> {
    let path = resolve_wallet_path(wallet_path)?;
    let password = rpassword::prompt_password("Wallet password: ")
        .map_err(|e| format!("Failed to read password: {}", e))?;

    let mut data = wallet_file::load_wallet(&path, &password)?;
    let mnemonic = &data.mnemonic;
    let network = &data.network;

    let daemon_height = monero_rust::get_daemon_height(daemon).await?;
    let start_height = data.last_sync_height;

    if start_height >= daemon_height {
        println!("Wallet is already synced to height {}", daemon_height);
        return Ok(());
    }

    let address = monero_rust::derive_address(mnemonic, network, "")?;
    println!("Syncing wallet: {}", address);
    println!(
        "Scanning from height {} to {} ({} blocks)",
        start_height,
        daemon_height,
        daemon_height - start_height
    );

    let total_blocks = daemon_height - start_height;
    let pb = ProgressBar::new(total_blocks);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} blocks ({eta})",
        )
        .unwrap()
        .progress_chars("=>-"),
    );

    let lookahead = monero_rust::DEFAULT_LOOKAHEAD;
    let mut current_height = start_height;
    let mut total_outputs_found: usize = 0;
    let mut cached_scanner: Option<monero_rust::CachedScanner> = None;

    loop {
        if current_height >= daemon_height {
            break;
        }

        let fetched =
            monero_rust::fetch_blocks_batch_with_url(daemon, current_height, false).await?;

        if fetched.is_empty() {
            break;
        }

        let (batch_results, returned_scanner) = monero_rust::process_fetched_batch_cached(
            fetched,
            mnemonic,
            network,
            lookahead,
            cached_scanner,
            "",
        )
        .await?;
        cached_scanner = Some(returned_scanner);

        let processed = monero_rust::process_single_wallet_batch(
            &batch_results,
            None, // scan all accounts
            daemon_height,
            current_height,
        );

        // Merge new outputs into wallet data, deduplicating by key_image
        // to handle interrupted syncs that may re-fetch overlapping blocks.
        let mut new_output_count = 0;
        for output in processed.outputs_to_store {
            if !data
                .outputs
                .iter()
                .any(|existing| existing.key_image == output.key_image)
            {
                data.outputs.push(output);
                new_output_count += 1;
            }
        }
        total_outputs_found += new_output_count;

        if !processed.spent_key_images.is_empty() {
            for ki in &processed.spent_key_images {
                for output in &mut data.outputs {
                    if output.key_image == *ki && !output.spent {
                        output.spent = true;
                        output.spent_height = Some(processed.batch_end_height);
                    }
                }
            }
        }

        current_height = processed.batch_end_height;
        data.last_sync_height = current_height;

        let scanned = current_height.saturating_sub(start_height);
        pb.set_position(scanned.min(total_blocks));

        // Save wallet after each batch so progress is not lost
        wallet_file::save_wallet(&path, &data, &password)?;

        if !processed.should_continue {
            break;
        }
    }

    pb.finish_with_message("done");

    let balance: u64 = data
        .outputs
        .iter()
        .filter(|o| !o.spent)
        .map(|o| o.amount)
        .sum();

    println!();
    println!("=== Sync Complete ===");
    println!("Synced to height: {}", current_height);
    println!("New outputs found: {}", total_outputs_found);
    println!("Total outputs: {}", data.outputs.len());
    println!(
        "Unspent outputs: {}",
        data.outputs.iter().filter(|o| !o.spent).count()
    );
    println!("Balance: {:.12} XMR", balance as f64 / 1_000_000_000_000.0);

    Ok(())
}

async fn cmd_transfer(
    address: &str,
    amount: &str,
    daemon: &str,
    wallet_path: Option<&str>,
) -> Result<(), String> {
    let path = resolve_wallet_path(wallet_path)?;
    let password = rpassword::prompt_password("Wallet password: ")
        .map_err(|e| format!("Failed to read password: {}", e))?;

    let mut data = wallet_file::load_wallet(&path, &password)?;
    if data.is_view_only() {
        return Err("Cannot transfer from a view-only wallet".to_string());
    }
    let seed = &data.mnemonic;
    let network = &data.network;

    let amount_pico = parse_xmr_to_piconero(&amount)?;
    if amount_pico == 0 {
        return Err("Amount must be positive".to_string());
    }

    let daemon_height = monero_rust::get_daemon_height(daemon).await?;
    let spendable: Vec<monero_rust::WalletOutput> = data
        .outputs
        .iter()
        .filter(|o| !o.spent && !o.frozen && monero_rust::is_spendable(o, daemon_height))
        .cloned()
        .collect();

    let selection = monero_rust::select_inputs(&spendable, amount_pico, 1, None)?;

    let stored_outputs: Vec<monero_rust::native::StoredOutputData> =
        selection.selected.iter().map(|o| o.into()).collect();

    let prepared = monero_rust::native::prepare_transaction(
        daemon,
        network,
        stored_outputs.clone(),
        &[(address.to_string(), amount_pico)],
    )
    .await?;

    println!("=== Transaction Summary ===");
    println!();
    println!("Destination: {}", address);
    println!(
        "Amount:      {:.12} XMR",
        amount_pico as f64 / 1_000_000_000_000.0
    );
    println!(
        "Fee:         {:.12} XMR",
        prepared.fee as f64 / 1_000_000_000_000.0
    );
    println!(
        "Total:       {:.12} XMR",
        (amount_pico + prepared.fee) as f64 / 1_000_000_000_000.0
    );
    println!("Inputs:      {}", selection.selected.len());
    println!();
    print!("Confirm transaction? (yes/no): ");
    use std::io::Write;
    std::io::stdout()
        .flush()
        .map_err(|e| format!("Failed to flush stdout: {}", e))?;

    let mut confirm = String::new();
    std::io::stdin()
        .read_line(&mut confirm)
        .map_err(|e| format!("Failed to read confirmation: {}", e))?;

    if confirm.trim().to_lowercase() != "yes" {
        println!("Transaction cancelled.");
        return Ok(());
    }

    println!("Signing transaction...");
    let result = monero_rust::native::create_transaction(
        daemon,
        seed,
        network,
        stored_outputs,
        &[(address.to_string(), amount_pico)],
    )
    .await?;

    println!("Broadcasting transaction...");
    monero_rust::native::broadcast_transaction(daemon, &result.tx_blob, false).await?;

    let spent_key_set: std::collections::HashSet<String> = selection
        .selected
        .iter()
        .map(|o| o.key_image.clone())
        .collect();
    for output in &mut data.outputs {
        if spent_key_set.contains(&output.key_image) && !output.spent {
            output.spent = true;
            output.spent_height = Some(daemon_height);
        }
    }

    for change in &result.change_outputs {
        data.outputs.push(monero_rust::WalletOutput {
            tx_hash: change.tx_hash.clone(),
            output_index: change.output_index,
            amount: change.amount,
            amount_xmr: change.amount_xmr.clone(),
            key: change.key.clone(),
            key_offset: change.key_offset.clone(),
            commitment_mask: change.commitment_mask.clone(),
            subaddress_index: change.subaddress_index,
            payment_id: None,
            received_output_bytes: change.received_output_bytes.clone(),
            block_height: daemon_height,
            spent: false,
            spent_height: None,
            key_image: change.key_image.clone(),
            is_coinbase: false,
            frozen: false,
        });
    }

    wallet_file::save_wallet(&path, &data, &password)?;

    println!();
    println!("=== Transaction Sent ===");
    println!("Transaction ID: {}", result.tx_id);
    println!(
        "Fee:            {:.12} XMR",
        result.fee as f64 / 1_000_000_000_000.0
    );

    Ok(())
}

fn cmd_info(wallet_path: Option<&str>) -> Result<(), String> {
    let path = resolve_wallet_path(wallet_path)?;
    let password = rpassword::prompt_password("Wallet password: ")
        .map_err(|e| format!("Failed to read password: {}", e))?;

    let data = wallet_file::load_wallet(&path, &password)?;

    let address = monero_rust::derive_address(&data.mnemonic, &data.network, "")?;
    let keys = monero_rust::derive_keys(&data.mnemonic, &data.network, "")?;

    let unspent_count = data.outputs.iter().filter(|o| !o.spent).count();

    println!("=== Wallet Info ===");
    println!();
    println!("Wallet file:      {}", path.display());
    println!("Network:          {}", data.network);
    println!("Primary address:  {}", address);
    println!("Public view key:  {}", keys.public_view_key);
    println!("Last sync height: {}", data.last_sync_height);
    println!("Total outputs:    {}", data.outputs.len());
    println!("Unspent outputs:  {}", unspent_count);

    Ok(())
}

async fn cmd_sweep_all(
    address: &str,
    daemon: &str,
    wallet_path: Option<&str>,
) -> Result<(), String> {
    let path = resolve_wallet_path(wallet_path)?;
    let password = rpassword::prompt_password("Wallet password: ")
        .map_err(|e| format!("Failed to read password: {}", e))?;

    let mut data = wallet_file::load_wallet(&path, &password)?;
    if data.is_view_only() {
        return Err("Cannot sweep from a view-only wallet".to_string());
    }
    let seed = &data.mnemonic;
    let network = &data.network;

    let daemon_height = monero_rust::get_daemon_height(daemon).await?;
    let spendable: Vec<monero_rust::WalletOutput> = data
        .outputs
        .iter()
        .filter(|o| !o.spent && !o.frozen && monero_rust::is_spendable(o, daemon_height))
        .cloned()
        .collect();

    if spendable.is_empty() {
        return Err("No spendable outputs available".to_string());
    }

    let total_amount: u64 = spendable.iter().map(|o| o.amount).sum();

    let stored_outputs: Vec<monero_rust::native::StoredOutputData> =
        spendable.iter().map(|o| o.into()).collect();

    let fee_est = monero_rust::estimate_fee(spendable.len(), 2);

    let send_amount = total_amount.saturating_sub(fee_est);
    if send_amount == 0 {
        return Err("Balance too low to cover the transaction fee. Cannot sweep.".to_string());
    }

    println!("=== Sweep All ===");
    println!();
    println!("Destination: {}", address);
    println!(
        "Balance:     {:.12} XMR",
        total_amount as f64 / 1_000_000_000_000.0
    );
    println!(
        "Est. fee:    {:.12} XMR",
        fee_est as f64 / 1_000_000_000_000.0
    );
    println!(
        "Send amount: {:.12} XMR (approx)",
        send_amount as f64 / 1_000_000_000_000.0
    );
    println!("Inputs:      {}", spendable.len());
    println!();
    print!("Confirm sweep? (yes/no): ");
    use std::io::Write;
    std::io::stdout()
        .flush()
        .map_err(|e| format!("Failed to flush stdout: {}", e))?;

    let mut confirm = String::new();
    std::io::stdin()
        .read_line(&mut confirm)
        .map_err(|e| format!("Failed to read confirmation: {}", e))?;

    if confirm.trim().to_lowercase() != "yes" {
        println!("Sweep cancelled.");
        return Ok(());
    }

    println!("Building sweep transaction...");
    let result =
        monero_rust::native::sweep_all(daemon, seed, network, stored_outputs, address).await?;

    println!("Broadcasting transaction...");
    monero_rust::native::broadcast_transaction(daemon, &result.tx_blob, false).await?;

    let spent_key_set: std::collections::HashSet<String> =
        spendable.iter().map(|o| o.key_image.clone()).collect();
    for output in &mut data.outputs {
        if spent_key_set.contains(&output.key_image) && !output.spent {
            output.spent = true;
            output.spent_height = Some(daemon_height);
        }
    }

    wallet_file::save_wallet(&path, &data, &password)?;

    println!();
    println!("=== Sweep Complete ===");
    println!("Transaction ID: {}", result.tx_id);
    println!(
        "Fee:            {:.12} XMR",
        result.fee as f64 / 1_000_000_000_000.0
    );

    Ok(())
}
