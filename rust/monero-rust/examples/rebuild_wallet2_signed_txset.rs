use std::{env, fs, path::PathBuf};

use monero_rust::{epee_compat, native};

struct Args {
    unsigned_txset: PathBuf,
    reference_signed_txset: PathBuf,
    spend_key_hex: String,
    view_key_hex: String,
    network: String,
    out: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut unsigned_txset: Option<PathBuf> = None;
    let mut reference_signed_txset: Option<PathBuf> = None;
    let mut spend_key_hex: Option<String> = None;
    let mut view_key_hex: Option<String> = None;
    let mut network = String::from("mainnet");
    let mut out: Option<PathBuf> = None;

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
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(Args {
        unsigned_txset: unsigned_txset
            .ok_or_else(|| "--unsigned-txset is required".to_string())?,
        reference_signed_txset: reference_signed_txset
            .ok_or_else(|| "--reference-signed-txset is required".to_string())?,
        spend_key_hex: spend_key_hex.ok_or_else(|| "--spend-key-hex is required".to_string())?,
        view_key_hex: view_key_hex.ok_or_else(|| "--view-key-hex is required".to_string())?,
        network,
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
    let signed = native::sign_unsigned_transaction_with_private_keys(
        spend_key,
        view_key,
        &hex::encode(&unsigned_txset),
        &args.network,
    )?;
    let tx_blob = hex::decode(&signed.tx_blob).map_err(|e| format!("invalid signed blob: {e}"))?;
    let rebuilt = epee_compat::build_signed_monero_txset(epee_compat::BuildSignedTxSetRequest {
        unsigned_txset: &unsigned_txset,
        view_secret_key: &view_key,
        tx_blob: &tx_blob,
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
