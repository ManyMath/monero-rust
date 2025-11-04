use monero_rust::{
    WalletState, TransactionConfig, TransactionPriority,
    Network, ConnectionConfig,
};

const STAGENET_RPC: &str = "http://stagenet.xmr-tw.org:38081";

const TEST_SEED: &str = "hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden";

const TEST_ADDRESS: &str = "569ubRY6tYfgF3VpxQumrUCRaEtdyyh6NG8sVD3YRVVJbK1jkpJ3zq8WHLijVzodQ22LxwkdWx7fS2a6JzaRGzkNU654PZu";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Sparse scanning demo");
    println!("====================\n");

    let mut wallet = WalletState::from_mnemonic(
        TEST_SEED,
        None,
        Network::Stagenet,
    )?;

    println!("Wallet created from seed");
    println!("Address: {}", wallet.get_address());

    let config = ConnectionConfig::new(STAGENET_RPC.to_string());
    wallet.connect(config).await?;
    println!("Connected to {}", STAGENET_RPC);

    let blocks_to_scan: Vec<u64> = vec![
        2032114, 2032323, 2032324, 2032326, 2032338, 2032598, 2034667, 2034681, 2034729 // 2034682
    ];
    println!("\nScanning {} specific blocks...", blocks_to_scan.len());

    wallet.scan_specific_blocks(&blocks_to_scan).await?;

    if let Some((scanned, total)) = wallet.get_sparse_progress() {
        println!("Progress: {}/{} blocks", scanned, total);
    }

    let balance = wallet.get_balance();
    let unlocked = wallet.get_unlocked_balance();
    println!("\nBalance: {} piconeros", balance);
    println!("Unlocked: {} piconeros", unlocked);

    let outputs = wallet.get_outputs(false)?;
    println!("Outputs: {}", outputs.len());

    if balance > 0 {
        println!("\n--- Transaction test ---");

        let dest = TEST_ADDRESS;
        let amount = 2_000_000_000; // 0.002 XMR

        let estimated_fee = wallet.estimate_tx_fee(1, TransactionPriority::Default).await?;
        println!("Estimated fee: {} piconeros", estimated_fee);

        if unlocked >= amount + estimated_fee {
            println!("Sufficient funds for transaction");

            let config = TransactionConfig::default();
            match wallet.create_tx(dest, amount, config).await {
                Ok(pending) => {
                    println!("Transaction created:");
                    println!("  TXID: {}", hex::encode(pending.txid()));
                    println!("  Fee: {} piconeros", pending.fee());
                    println!("  Inputs: {}", pending.num_inputs());

                    println!("Broadcasting...");
                    match wallet.commit_tx(&pending).await {
                        Ok(txid) => println!("Broadcast success: {}", hex::encode(txid)),
                        Err(e) => println!("Broadcast failed: {}", e),
                    }
                }
                Err(e) => println!("Failed to create tx: {}", e),
            }
        } else {
            println!("Insufficient unlocked balance for demo transaction");
        }
    }

    wallet.disconnect().await;
    println!("\nDone.");
    Ok(())
}
