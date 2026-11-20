use std::{env, fs, path::PathBuf};

use monero_rust::{epee_compat, native};

struct Args {
    unsigned_txset: PathBuf,
    reference_signed_txset: PathBuf,
    spend_key_hex: String,
    view_key_hex: String,
    network: String,
    out: PathBuf,
    approved_review: Option<PathBuf>,
    max_fee: Option<u64>,
    max_wallet_debit: Option<u64>,
}

fn parse_args() -> Result<Args, String> {
    let mut unsigned_txset: Option<PathBuf> = None;
    let mut reference_signed_txset: Option<PathBuf> = None;
    let mut spend_key_hex: Option<String> = None;
    let mut view_key_hex: Option<String> = None;
    let mut network = String::from("mainnet");
    let mut out: Option<PathBuf> = None;
    let mut approved_review = None;
    let mut max_fee = None;
    let mut max_wallet_debit = None;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("{arg} requires a value"))?;
        match arg.as_str() {
            "--unsigned-txset" => unsigned_txset = Some(PathBuf::from(value)),
            "--reference-signed-txset" => reference_signed_txset = Some(PathBuf::from(value)),
            "--spend-key-hex" => spend_key_hex = Some(value),
            "--view-key-hex" => view_key_hex = Some(value),
            "--network" => {
                network = value;
            }
            "--out" => out = Some(PathBuf::from(value)),
            "--approved-review" => approved_review = Some(PathBuf::from(value)),
            "--max-fee" => max_fee = Some(value.parse::<u64>().map_err(|e| e.to_string())?),
            "--max-wallet-debit" => {
                max_wallet_debit = Some(value.parse::<u64>().map_err(|e| e.to_string())?)
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(Args {
        unsigned_txset: unsigned_txset.ok_or_else(|| "--unsigned-txset is required".to_string())?,
        reference_signed_txset: reference_signed_txset
            .ok_or_else(|| "--reference-signed-txset is required".to_string())?,
        spend_key_hex: spend_key_hex.ok_or_else(|| "--spend-key-hex is required".to_string())?,
        view_key_hex: view_key_hex.ok_or_else(|| "--view-key-hex is required".to_string())?,
        network,
        approved_review,
        max_fee,
        max_wallet_debit,
        out: out.ok_or_else(|| "--out is required".to_string())?,
    })
}

fn decode_key(hex_value: &str, label: &str) -> Result<[u8; 32], String> {
    let bytes = hex::decode(hex_value.trim()).map_err(|e| format!("invalid {label}: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| format!("{label} must be exactly 32 bytes"))
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let unsigned_txset = fs::read(&args.unsigned_txset)
        .map_err(|e| format!("failed to read unsigned txset: {e}"))?;
    let reference_signed_txset = fs::read(&args.reference_signed_txset)
        .map_err(|e| format!("failed to read reference signed txset: {e}"))?;
    let spend_key = decode_key(&args.spend_key_hex, "spend key")?;
    let view_key = decode_key(&args.view_key_hex, "view key")?;

    let reference_summary =
        epee_compat::parse_signed_monero_txset_summary(&reference_signed_txset, &view_key)?;
    let preview = native::preview_unsigned_transaction(
        spend_key,
        view_key,
        &hex::encode(&unsigned_txset),
        &args.network,
    )?;
    // First run emits the full review. A subsequent run supplies the reviewed JSON
    // and independent fee/debit limits, all checked against the exact input bytes.
    println!(
        "{}",
        serde_json::to_string_pretty(&preview).map_err(|e| e.to_string())?
    );
    let review_path = args.approved_review.ok_or("review the JSON, then provide --approved-review, --max-fee and --max-wallet-debit (piconero)")?;
    let approved: native::TransactionPreview =
        serde_json::from_slice(&fs::read(review_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let approval = approved.approve(
        args.max_fee.ok_or("--max-fee is required")?,
        args.max_wallet_debit
            .ok_or("--max-wallet-debit is required")?,
    )?;
    let signed = native::sign_unsigned_transaction_with_private_keys(
        spend_key,
        view_key,
        &hex::encode(&unsigned_txset),
        &args.network,
        &approval,
    )?;
    let tx_blob = hex::decode(&signed.tx_blob).map_err(|e| format!("invalid signed blob: {e}"))?;
    let rebuilt = epee_compat::build_signed_monero_txset(epee_compat::BuildSignedTxSetRequest {
        unsigned_txset: &unsigned_txset,
        view_secret_key: &view_key,
        tx_blobs: vec![&tx_blob],
        key_images: &reference_summary.key_images,
        tx_key_images: &reference_summary.tx_key_images,
    })?;

    fs::write(&args.out, &rebuilt).map_err(|e| format!("failed to write rebuilt txset: {e}"))?;
    println!(
        "rebuilt signed_monero_tx {} bytes tx_id {}",
        rebuilt.len(),
        signed.tx_id
    );
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
