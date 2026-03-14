//! Key image export/import with support for both legacy EPEE and Monero v3 formats.
//!
//! The old EPEE-based export format is deprecated but retained for backward
//! compatibility. New exports should use `key_image_signing::export_key_images_v3`
//! for full interoperability with Feather Wallet and monero-wallet-cli.
//!
//! The `import_key_images` function auto-detects the format: it tries EPEE first,
//! and if that fails, falls back to the v3 encrypted binary format (requires
//! the view secret key for decryption).

use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};

use monero_serai::transaction::{Input, Transaction};

pub const UNSIGNED_TX_MAGIC: &[u8] = b"Monero unsigned tx set\x05";
pub const SIGNED_TX_MAGIC: &[u8] = b"Monero signed tx set\x05";
pub const KEY_IMAGES_MAGIC: &[u8] = b"Monero key image export\x03";
pub const OUTPUT_EXPORT_MAGIC: &[u8] = b"Monero output export\x04";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputExportPreview {
    pub public_spend_key: [u8; 32],
    pub public_view_key: [u8; 32],
    pub archive_body_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoneroTxSetInspection {
    pub kind: MoneroTxSetKind,
    pub encrypted_payload_len: usize,
    pub decrypted_archive_len: usize,
    pub archive_version: u64,
    pub transaction_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUnsignedTxSetSummary {
    pub archive_version: u64,
    pub txes: Vec<TxConstructionDataSummary>,
    pub new_transfer_first: u64,
    pub new_transfer_second: u64,
    pub new_transfers: Vec<ExportedTransferDetails>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSignedTxSetSummary {
    pub archive_version: u64,
    pub ptxes: Vec<PendingTxSummary>,
    pub key_image_count: u64,
    pub key_images: Vec<[u8; 32]>,
    pub tx_key_image_count: u64,
    pub tx_key_images: Vec<SignedTxSetKeyImagePair>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedSignedTx {
    pub tx_hash: [u8; 32],
    pub tx_blob: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedTxSetKeyImagePair {
    pub public_key: [u8; 32],
    pub key_image: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildSignedTxSetRequest<'a> {
    pub unsigned_txset: &'a [u8],
    pub view_secret_key: &'a [u8; 32],
    pub tx_blobs: Vec<&'a [u8]>,
    pub key_images: &'a [[u8; 32]],
    pub tx_key_images: &'a [SignedTxSetKeyImagePair],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTxSummary {
    pub tx_hash: [u8; 32],
    pub tx_blob: Vec<u8>,
    pub tx_version: u64,
    pub tx_unlock_time: u64,
    pub tx_input_count: u64,
    pub tx_input_ring_sizes: Vec<u64>,
    pub tx_output_count: u64,
    pub tx_extra_len: usize,
    pub rct_type: Option<u8>,
    pub rct_fee: Option<u64>,
    pub dust: u64,
    pub fee: u64,
    pub dust_added_to_fee: bool,
    pub change_amount: u64,
    pub selected_transfer_count: u64,
    pub selected_transfer_indices: Vec<u64>,
    pub key_images_len: usize,
    pub key_images_blob: Vec<u8>,
    pub tx_key_is_zero: bool,
    pub tx_key: [u8; 32],
    pub additional_tx_key_count: u64,
    pub additional_tx_keys: Vec<[u8; 32]>,
    pub destination_count: u64,
    pub destination_total_amount: u64,
    pub destinations: Vec<TxDestinationEntrySummary>,
    pub construction: TxConstructionDataSummary,
    pub multisig_sig_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TransactionSummary {
    hash: [u8; 32],
    blob: Vec<u8>,
    version: u64,
    unlock_time: u64,
    input_count: u64,
    input_ring_sizes: Vec<u64>,
    output_count: u64,
    extra_len: usize,
    rct_type: Option<u8>,
    rct_fee: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxConstructionDataSummary {
    pub source_count: u64,
    pub source_ring_sizes: Vec<u64>,
    pub sources: Vec<TxSourceEntrySummary>,
    pub change_amount: u64,
    pub change: TxDestinationEntrySummary,
    pub split_destination_count: u64,
    pub split_destination_total_amount: u64,
    pub split_destinations: Vec<TxDestinationEntrySummary>,
    pub selected_transfer_count: u64,
    pub selected_transfer_indices: Vec<u64>,
    pub extra_len: usize,
    pub extra: Vec<u8>,
    pub unlock_time: u64,
    pub construction_flags: u8,
    pub use_rct: bool,
    pub use_view_tags: bool,
    pub rct_range_proof_type: u64,
    pub rct_bp_version: u64,
    pub destination_count: u64,
    pub destination_total_amount: u64,
    pub destinations: Vec<TxDestinationEntrySummary>,
    pub subaddr_account: u32,
    pub subaddr_indices: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxSourceEntrySummary {
    pub ring: Vec<TxSourceOutputSummary>,
    pub real_output: u64,
    pub real_out_tx_key: [u8; 32],
    pub real_out_additional_tx_keys: Vec<[u8; 32]>,
    pub real_output_in_tx_index: u64,
    pub amount: u64,
    pub rct: bool,
    pub mask: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxSourceOutputSummary {
    pub global_output_index: u64,
    pub output_public_key: [u8; 32],
    pub commitment: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxDestinationEntrySummary {
    pub original_len: usize,
    pub original: Vec<u8>,
    pub amount: u64,
    pub spend_public_key: [u8; 32],
    pub view_public_key: [u8; 32],
    pub is_subaddress: bool,
    pub is_integrated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedOutputExport {
    pub public_spend_key: [u8; 32],
    pub public_view_key: [u8; 32],
    pub offset: u64,
    pub total_outputs: u64,
    pub outputs: Vec<ExportedTransferDetails>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedTransferDetails {
    pub output_public_key: [u8; 32],
    pub internal_output_index: u64,
    pub global_output_index: u64,
    pub tx_public_key: [u8; 32],
    pub flags: ExportedTransferFlags,
    pub amount: u64,
    pub additional_tx_keys: Vec<[u8; 32]>,
    pub subaddress_major: u64,
    pub subaddress_minor: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportedTransferFlags {
    pub raw: u8,
    pub spent: bool,
    pub frozen: bool,
    pub rct: bool,
    pub key_image_known: bool,
    pub key_image_request: bool,
    pub key_image_partial: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoneroTxSetKind {
    Unsigned,
    Signed,
}

impl MoneroTxSetKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unsigned => "Monero unsigned tx set",
            Self::Signed => "Monero signed tx set",
        }
    }
}

pub fn inspect_output_export(
    data: &[u8],
    view_secret_key: &[u8; 32],
) -> Result<OutputExportPreview, String> {
    let payload = strip_magic(OUTPUT_EXPORT_MAGIC, data)?;
    let plaintext = crate::key_image_signing::decrypt_with_view_key(payload, view_secret_key)?;
    if plaintext.len() < 64 {
        return Err("Decrypted output export is too short for account header".to_string());
    }

    let mut public_spend_key = [0u8; 32];
    public_spend_key.copy_from_slice(&plaintext[..32]);
    let mut public_view_key = [0u8; 32];
    public_view_key.copy_from_slice(&plaintext[32..64]);

    Ok(OutputExportPreview {
        public_spend_key,
        public_view_key,
        archive_body_len: plaintext.len() - 64,
    })
}

pub fn parse_output_export(
    data: &[u8],
    view_secret_key: &[u8; 32],
) -> Result<ParsedOutputExport, String> {
    let payload = strip_magic(OUTPUT_EXPORT_MAGIC, data)?;
    let plaintext = crate::key_image_signing::decrypt_with_view_key(payload, view_secret_key)?;
    if plaintext.len() < 64 {
        return Err("Decrypted output export is too short for account header".to_string());
    }

    let mut public_spend_key = [0u8; 32];
    public_spend_key.copy_from_slice(&plaintext[..32]);
    let mut public_view_key = [0u8; 32];
    public_view_key.copy_from_slice(&plaintext[32..64]);

    let archive = BinaryArchiveReader::new(&plaintext[64..]);
    let (offset, total_outputs, outputs) = archive.read_output_export_body()?;

    Ok(ParsedOutputExport {
        public_spend_key,
        public_view_key,
        offset,
        total_outputs,
        outputs,
    })
}

pub fn detect_monero_txset(data: &[u8]) -> Option<MoneroTxSetKind> {
    if data.starts_with(UNSIGNED_TX_MAGIC) {
        Some(MoneroTxSetKind::Unsigned)
    } else if data.starts_with(SIGNED_TX_MAGIC) {
        Some(MoneroTxSetKind::Signed)
    } else {
        None
    }
}

pub fn inspect_monero_txset(
    data: &[u8],
    view_secret_key: &[u8; 32],
) -> Result<MoneroTxSetInspection, String> {
    let (kind, magic) = if data.starts_with(UNSIGNED_TX_MAGIC) {
        (MoneroTxSetKind::Unsigned, UNSIGNED_TX_MAGIC)
    } else if data.starts_with(SIGNED_TX_MAGIC) {
        (MoneroTxSetKind::Signed, SIGNED_TX_MAGIC)
    } else {
        return Err("Not a supported Monero wallet2 txset container".to_string());
    };

    let encrypted = strip_magic(magic, data)?;
    let decrypted = crate::key_image_signing::decrypt_with_view_key(encrypted, view_secret_key)?;
    let (archive_version, transaction_count) =
        BinaryArchiveReader::new(&decrypted).read_monero_txset_prefix(kind)?;

    Ok(MoneroTxSetInspection {
        kind,
        encrypted_payload_len: encrypted.len(),
        decrypted_archive_len: decrypted.len(),
        archive_version,
        transaction_count,
    })
}

pub fn parse_unsigned_monero_txset_summary(
    data: &[u8],
    view_secret_key: &[u8; 32],
) -> Result<ParsedUnsignedTxSetSummary, String> {
    let encrypted = strip_magic(UNSIGNED_TX_MAGIC, data)?;
    let decrypted = crate::key_image_signing::decrypt_with_view_key(encrypted, view_secret_key)?;
    BinaryArchiveReader::new(&decrypted).read_unsigned_txset_summary()
}

pub fn parse_signed_monero_txset_summary(
    data: &[u8],
    view_secret_key: &[u8; 32],
) -> Result<ParsedSignedTxSetSummary, String> {
    let encrypted = strip_magic(SIGNED_TX_MAGIC, data)?;
    let decrypted = crate::key_image_signing::decrypt_with_view_key(encrypted, view_secret_key)?;
    BinaryArchiveReader::new(&decrypted).read_signed_txset_summary()
}

pub fn extract_signed_monero_txset_transactions(
    data: &[u8],
    view_secret_key: &[u8; 32],
) -> Result<Vec<ExtractedSignedTx>, String> {
    let summary = parse_signed_monero_txset_summary(data, view_secret_key)?;
    Ok(summary
        .ptxes
        .into_iter()
        .map(|ptx| ExtractedSignedTx {
            tx_hash: ptx.tx_hash,
            tx_blob: ptx.tx_blob,
        })
        .collect())
}

pub fn build_signed_monero_txset(request: BuildSignedTxSetRequest<'_>) -> Result<Vec<u8>, String> {
    let unsigned =
        parse_unsigned_monero_txset_summary(request.unsigned_txset, request.view_secret_key)?;
    if request.tx_blobs.len() != unsigned.txes.len() {
        return Err(format!(
            "Wallet2 signed txset builder expected {} signed transaction blobs, got {}",
            unsigned.txes.len(),
            request.tx_blobs.len()
        ));
    }

    struct Pending<'a> {
        tx_blob: &'a [u8],
        fee: u64,
        construction: &'a TxConstructionDataSummary,
        key_images_text: String,
    }

    let mut pending = Vec::with_capacity(unsigned.txes.len());
    for (index, (construction, tx_blob)) in unsigned
        .txes
        .iter()
        .zip(request.tx_blobs.iter())
        .enumerate()
    {
        let mut cursor = Cursor::new(*tx_blob);
        let tx = Transaction::read(&mut cursor)
            .map_err(|e| format!("Signed transaction blob #{index} is not valid: {e:?}"))?;
        let mut trailing = [0u8; 1];
        if cursor
            .read(&mut trailing)
            .map_err(|e| format!("Failed to validate signed transaction cursor #{index}: {e:?}"))?
            != 0
        {
            return Err(format!(
                "Signed transaction blob #{index} has trailing bytes"
            ));
        }
        if tx.serialize() != *tx_blob {
            return Err(format!("Signed transaction blob #{index} is not canonical"));
        }

        if let Some(max_selected) = construction.selected_transfer_indices.iter().max() {
            let required_len = max_selected
                .checked_add(1)
                .ok_or_else(|| "Wallet2 selected transfer index overflow".to_string())?;
            let actual_len: u64 = request
                .key_images
                .len()
                .try_into()
                .map_err(|_| "Wallet2 key image vector length exceeds u64".to_string())?;
            if actual_len < required_len {
                return Err(format!(
                    "Wallet2 signed txset key_images must include entries through selected transfer index {max_selected}, got {actual_len}"
                ));
            }
        }

        let total_input = construction
            .sources
            .iter()
            .try_fold(0u64, |acc, source| acc.checked_add(source.amount))
            .ok_or_else(|| "Wallet2 source amount overflow".to_string())?;
        let total_output = construction.split_destination_total_amount;
        let fee = total_input
            .checked_sub(total_output)
            .ok_or_else(|| "Wallet2 outputs exceed inputs".to_string())?;

        let spent_key_images = tx
            .prefix
            .inputs
            .iter()
            .map(|input| match input {
                Input::ToKey { key_image, .. } => Ok(key_image.compress().to_bytes()),
                Input::Gen(_) => {
                    Err("Signed wallet2 txset cannot contain miner inputs".to_string())
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        if construction.selected_transfer_indices.len() != spent_key_images.len() {
            return Err(format!(
                "Wallet2 selected transfer count {} does not match signed transaction #{index} input count {}",
                construction.selected_transfer_indices.len(),
                spent_key_images.len()
            ));
        }
        let mut remaining_spent_key_images = spent_key_images.clone();
        for selected_transfer_index in construction.selected_transfer_indices.iter().copied() {
            let selected_transfer_index: usize = selected_transfer_index
                .try_into()
                .map_err(|_| "Wallet2 selected transfer index exceeds usize".to_string())?;
            let Some(expected_key_image) = request.key_images.get(selected_transfer_index) else {
                return Err(format!(
                    "Wallet2 signed txset key_images missing selected transfer index {selected_transfer_index}"
                ));
            };
            let Some(position) = remaining_spent_key_images
                .iter()
                .position(|spent_key_image| spent_key_image == expected_key_image)
            else {
                return Err(format!(
                    "Wallet2 signed txset key image at selected transfer index {selected_transfer_index} was not spent by signed transaction #{index}"
                ));
            };
            remaining_spent_key_images.remove(position);
        }

        let key_images_text = spent_key_images
            .iter()
            .map(hex::encode)
            .collect::<Vec<_>>()
            .join(" ");
        let key_images_text = if key_images_text.is_empty() {
            key_images_text
        } else {
            format!("{key_images_text} ")
        };
        pending.push(Pending {
            tx_blob,
            fee,
            construction,
            key_images_text,
        });
    }

    let mut archive = BinaryArchiveWriter::new();
    archive.write_varint(0); // signed_tx_set version
    archive.write_varint(pending.len() as u64); // ptx count
    for ptx in pending {
        archive.write_pending_tx(
            ptx.tx_blob,
            ptx.fee,
            ptx.construction,
            ptx.key_images_text.as_bytes(),
        )?;
    }
    archive.write_varint(request.key_images.len() as u64);
    for key_image in request.key_images {
        archive.write_fixed(key_image);
    }
    archive.write_varint(request.tx_key_images.len() as u64);
    for pair in request.tx_key_images {
        archive.write_varint(2);
        archive.write_fixed(&pair.public_key);
        archive.write_fixed(&pair.key_image);
    }

    let encrypted = crate::key_image_signing::encrypt_with_view_key(
        &archive.into_inner(),
        request.view_secret_key,
    );
    Ok(wrap_with_magic(SIGNED_TX_MAGIC, &encrypted))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportedKeyImage {
    pub key_image: String,
    pub tx_hash: String,
    pub output_index: u8,
}

pub fn wrap_with_magic(magic: &[u8], data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(magic.len() + data.len());
    result.extend_from_slice(magic);
    result.extend_from_slice(data);
    result
}

pub fn strip_magic<'a>(magic: &[u8], data: &'a [u8]) -> Result<&'a [u8], String> {
    if data.len() < magic.len() {
        return Err("Data too short for magic header".to_string());
    }
    if &data[..magic.len()] != magic {
        return Err("Magic header mismatch".to_string());
    }
    Ok(&data[magic.len()..])
}

struct BinaryArchiveReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

struct BinaryArchiveWriter {
    bytes: Vec<u8>,
}

impl BinaryArchiveWriter {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn into_inner(self) -> Vec<u8> {
        self.bytes
    }

    fn write_pending_tx(
        &mut self,
        tx_blob: &[u8],
        fee: u64,
        construction: &TxConstructionDataSummary,
        key_images: &[u8],
    ) -> Result<(), String> {
        self.write_varint(1); // pending_tx version
        self.write_bytes(tx_blob);
        self.write_u64_le(0); // dust
        self.write_u64_le(fee);
        self.write_bool(false); // dust_added_to_fee
        self.write_destination(&construction.change)?;
        self.write_varint(construction.selected_transfer_indices.len() as u64);
        for index in &construction.selected_transfer_indices {
            self.write_varint(*index);
        }
        self.write_var_bytes(key_images);
        let mut hidden_tx_key = [0u8; 32];
        hidden_tx_key[0] = 1; // Monero uses rct::identity() here to hide the real tx key.
        self.write_fixed(&hidden_tx_key);
        self.write_varint(0); // additional_tx_keys
        self.write_varint(construction.destinations.len() as u64);
        for destination in &construction.destinations {
            self.write_destination(destination)?;
        }
        self.write_construction(construction)?;
        self.write_varint(0); // multisig_sigs
        self.write_fixed(&[0u8; 32]); // multisig_tx_key_entropy
        Ok(())
    }

    fn write_construction(
        &mut self,
        construction: &TxConstructionDataSummary,
    ) -> Result<(), String> {
        self.write_varint(construction.sources.len() as u64);
        for source in &construction.sources {
            self.write_source(source)?;
        }
        self.write_destination(&construction.change)?;
        self.write_varint(construction.split_destinations.len() as u64);
        for destination in &construction.split_destinations {
            self.write_destination(destination)?;
        }
        self.write_varint(construction.selected_transfer_indices.len() as u64);
        for index in &construction.selected_transfer_indices {
            self.write_varint(*index);
        }
        self.write_var_bytes(&construction.extra);
        self.write_u64_le(construction.unlock_time);
        self.write_u8(construction.construction_flags);
        self.write_varint(0); // rct_config version
        self.write_varint(construction.rct_range_proof_type);
        self.write_varint(construction.rct_bp_version);
        self.write_varint(construction.destinations.len() as u64);
        for destination in &construction.destinations {
            self.write_destination(destination)?;
        }
        self.write_u32_le(construction.subaddr_account);
        self.write_varint(construction.subaddr_indices.len() as u64);
        for index in &construction.subaddr_indices {
            self.write_varint(u64::from(*index));
        }
        Ok(())
    }

    fn write_source(&mut self, source: &TxSourceEntrySummary) -> Result<(), String> {
        self.write_varint(source.ring.len() as u64);
        for output in &source.ring {
            self.write_varint(2);
            self.write_varint(output.global_output_index);
            self.write_fixed(&output.output_public_key);
            self.write_fixed(&output.commitment);
        }
        self.write_u64_le(source.real_output);
        self.write_fixed(&source.real_out_tx_key);
        self.write_varint(source.real_out_additional_tx_keys.len() as u64);
        for key in &source.real_out_additional_tx_keys {
            self.write_fixed(key);
        }
        self.write_u64_le(source.real_output_in_tx_index);
        self.write_u64_le(source.amount);
        self.write_bool(source.rct);
        self.write_fixed(&source.mask);
        self.write_fixed(&[0u8; 128]); // rct::multisig_kLRki
        Ok(())
    }

    fn write_destination(&mut self, destination: &TxDestinationEntrySummary) -> Result<(), String> {
        self.write_var_bytes(&destination.original);
        self.write_varint(destination.amount);
        self.write_fixed(&destination.spend_public_key);
        self.write_fixed(&destination.view_public_key);
        self.write_bool(destination.is_subaddress);
        self.write_bool(destination.is_integrated);
        Ok(())
    }

    fn write_fixed(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    fn write_var_bytes(&mut self, bytes: &[u8]) {
        self.write_varint(bytes.len() as u64);
        self.write_fixed(bytes);
    }

    fn write_u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn write_bool(&mut self, value: bool) {
        self.write_u8(u8::from(value));
    }

    fn write_u32_le(&mut self, value: u32) {
        self.write_fixed(&value.to_le_bytes());
    }

    fn write_u64_le(&mut self, value: u64) {
        self.write_fixed(&value.to_le_bytes());
    }

    fn write_varint(&mut self, mut value: u64) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            self.write_u8(byte);
            if value == 0 {
                break;
            }
        }
    }
}

impl<'a> BinaryArchiveReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn read_output_export_body(
        mut self,
    ) -> Result<(u64, u64, Vec<ExportedTransferDetails>), String> {
        let tuple_size = self.read_varint()?;
        if tuple_size != 3 {
            return Err(format!("Unexpected output export tuple size: {tuple_size}"));
        }

        let offset = self.read_varint()?;
        let total_outputs = self.read_varint()?;
        let output_count = self.read_varint()?;
        let output_count: usize = output_count
            .try_into()
            .map_err(|_| "Output export record count exceeds usize".to_string())?;

        let mut outputs = Vec::with_capacity(output_count);
        for index in 0..output_count {
            outputs.push(
                self.read_exported_transfer_details()
                    .map_err(|e| format!("Failed to parse output export record #{index}: {e}"))?,
            );
        }

        if self.offset != self.bytes.len() {
            return Err(format!(
                "Output export archive has {} trailing bytes",
                self.bytes.len() - self.offset
            ));
        }

        Ok((offset, total_outputs, outputs))
    }

    fn read_monero_txset_prefix(mut self, kind: MoneroTxSetKind) -> Result<(u64, u64), String> {
        let version = self.read_varint()?;
        let expected_version = match kind {
            MoneroTxSetKind::Unsigned => 2,
            MoneroTxSetKind::Signed => 0,
        };
        if version != expected_version {
            return Err(format!(
                "Unsupported {} wallet2 archive version: {version}",
                kind.label()
            ));
        }

        let transaction_count = self.read_varint()?;
        Ok((version, transaction_count))
    }

    fn read_unsigned_txset_summary(mut self) -> Result<ParsedUnsignedTxSetSummary, String> {
        let archive_version = self.read_varint()?;
        if archive_version != 2 {
            return Err(format!(
                "Unsupported Monero unsigned tx set wallet2 archive version: {archive_version}"
            ));
        }

        let tx_count = self.read_varint()?;
        let tx_count_usize: usize = tx_count
            .try_into()
            .map_err(|_| "Unsigned txset transaction count exceeds usize".to_string())?;
        let mut txes = Vec::with_capacity(tx_count_usize);
        for index in 0..tx_count_usize {
            txes.push(
                self.read_tx_construction_data_summary().map_err(|e| {
                    format!("Failed to parse unsigned tx construction #{index}: {e}")
                })?,
            );
        }

        let tuple_size = self.read_varint()?;
        if tuple_size != 3 {
            return Err(format!(
                "Unexpected unsigned txset new_transfers tuple size: {tuple_size}"
            ));
        }
        let new_transfer_first = self.read_varint()?;
        let new_transfer_second = self.read_varint()?;
        let transfer_count = self.read_varint()?;
        let transfer_count_usize: usize = transfer_count
            .try_into()
            .map_err(|_| "Unsigned txset new transfer count exceeds usize".to_string())?;
        let mut new_transfers = Vec::with_capacity(transfer_count_usize);
        for index in 0..transfer_count_usize {
            new_transfers.push(self.read_exported_transfer_details().map_err(|e| {
                format!("Failed to parse unsigned txset new transfer #{index}: {e}")
            })?);
        }

        if self.offset != self.bytes.len() {
            return Err(format!(
                "Unsigned txset archive has {} trailing bytes",
                self.bytes.len() - self.offset
            ));
        }

        Ok(ParsedUnsignedTxSetSummary {
            archive_version,
            txes,
            new_transfer_first,
            new_transfer_second,
            new_transfers,
        })
    }

    fn read_signed_txset_summary(mut self) -> Result<ParsedSignedTxSetSummary, String> {
        let archive_version = self.read_varint()?;
        if archive_version != 0 {
            return Err(format!(
                "Unsupported Monero signed tx set wallet2 archive version: {archive_version}"
            ));
        }

        let ptx_count = self.read_varint()?;
        let ptx_count_usize: usize = ptx_count
            .try_into()
            .map_err(|_| "Signed txset pending transaction count exceeds usize".to_string())?;
        let mut ptxes = Vec::with_capacity(ptx_count_usize);
        for index in 0..ptx_count_usize {
            ptxes.push(
                self.read_pending_tx_summary()
                    .map_err(|e| format!("Failed to parse signed pending tx #{index}: {e}"))?,
            );
        }

        let key_image_count = self.read_varint()?;
        let key_image_count_usize: usize = key_image_count
            .try_into()
            .map_err(|_| "Signed txset key image count exceeds usize".to_string())?;
        let mut key_images = Vec::with_capacity(key_image_count_usize);
        for _ in 0..key_image_count_usize {
            key_images.push(self.read_fixed()?);
        }

        let tx_key_image_count = self.read_varint()?;
        let tx_key_image_count_usize: usize = tx_key_image_count
            .try_into()
            .map_err(|_| "Signed txset tx key image count exceeds usize".to_string())?;
        let mut tx_key_images = Vec::with_capacity(tx_key_image_count_usize);
        for index in 0..tx_key_image_count_usize {
            let pair_size = self.read_varint()?;
            if pair_size != 2 {
                return Err(format!(
                    "Unexpected signed txset tx key image pair size at #{index}: {pair_size}"
                ));
            }
            tx_key_images.push(SignedTxSetKeyImagePair {
                public_key: self.read_fixed()?,
                key_image: self.read_fixed()?,
            });
        }

        if self.offset != self.bytes.len() {
            return Err(format!(
                "Signed txset archive has {} trailing bytes",
                self.bytes.len() - self.offset
            ));
        }

        Ok(ParsedSignedTxSetSummary {
            archive_version,
            ptxes,
            key_image_count,
            key_images,
            tx_key_image_count,
            tx_key_images,
        })
    }

    fn read_pending_tx_summary(&mut self) -> Result<PendingTxSummary, String> {
        let pending_tx_version = self.read_varint()?;
        if pending_tx_version != 1 {
            return Err(format!(
                "Unsupported signed tx pending_tx version: {pending_tx_version}"
            ));
        }

        let tx = self.read_transaction_summary()?;
        let dust = self.read_u64_le()?;
        let fee = self.read_u64_le()?;
        let dust_added_to_fee = self.read_bool()?;
        let change = self.read_tx_destination_entry_summary()?;
        let change_amount = change.amount;

        let selected_transfer_count = self.read_varint()?;
        let selected_transfer_count_usize: usize = selected_transfer_count
            .try_into()
            .map_err(|_| "Signed tx selected transfer count exceeds usize".to_string())?;
        let mut selected_transfer_indices = Vec::with_capacity(selected_transfer_count_usize);
        for _ in 0..selected_transfer_count_usize {
            selected_transfer_indices.push(self.read_varint()?);
        }

        let key_images_blob = self.read_var_bytes()?;
        let key_images_len = key_images_blob.len();
        let tx_key = self.read_fixed::<32>()?;
        let tx_key_is_zero = tx_key.iter().all(|byte| *byte == 0);

        let additional_tx_key_count = self.read_varint()?;
        let additional_tx_key_count_usize: usize = additional_tx_key_count
            .try_into()
            .map_err(|_| "Additional tx key count exceeds usize".to_string())?;
        let mut additional_tx_keys = Vec::with_capacity(additional_tx_key_count_usize);
        for _ in 0..additional_tx_key_count_usize {
            additional_tx_keys.push(self.read_fixed()?);
        }

        let destination_count = self.read_varint()?;
        let destination_count_usize: usize = destination_count
            .try_into()
            .map_err(|_| "Signed tx destination count exceeds usize".to_string())?;
        let mut destination_total_amount = 0u64;
        let mut destinations = Vec::with_capacity(destination_count_usize);
        for index in 0..destination_count_usize {
            let amount = self
                .read_tx_destination_entry_summary()
                .map_err(|e| format!("Failed to parse signed tx destination #{index}: {e}"))?;
            destination_total_amount = destination_total_amount
                .checked_add(amount.amount)
                .ok_or_else(|| "Signed tx destination total amount overflow".to_string())?;
            destinations.push(amount);
        }

        let construction = self.read_tx_construction_data_summary()?;

        let multisig_sig_count = self.read_varint()?;
        if multisig_sig_count != 0 {
            return Err(format!(
                "Unsupported signed tx multisig signature count: {multisig_sig_count}"
            ));
        }

        self.skip_bytes(32)?; // multisig_tx_key_entropy

        Ok(PendingTxSummary {
            tx_version: tx.version,
            tx_unlock_time: tx.unlock_time,
            tx_input_count: tx.input_count,
            tx_input_ring_sizes: tx.input_ring_sizes,
            tx_output_count: tx.output_count,
            tx_extra_len: tx.extra_len,
            rct_type: tx.rct_type,
            rct_fee: tx.rct_fee,
            dust,
            fee,
            dust_added_to_fee,
            change_amount,
            selected_transfer_count,
            selected_transfer_indices,
            key_images_len,
            key_images_blob,
            tx_key_is_zero,
            tx_key,
            additional_tx_key_count,
            additional_tx_keys,
            destination_count,
            destination_total_amount,
            destinations,
            construction,
            multisig_sig_count,
            tx_hash: tx.hash,
            tx_blob: tx.blob,
        })
    }

    fn read_transaction_summary(&mut self) -> Result<TransactionSummary, String> {
        let tx_start = self.offset;
        let version = self.read_varint()?;
        if version == 0 || version > 2 {
            return Err(format!(
                "Unsupported transaction version in signed txset: {version}"
            ));
        }
        let unlock_time = self.read_varint()?;

        let input_count = self.read_varint()?;
        let input_count_usize: usize = input_count
            .try_into()
            .map_err(|_| "Transaction input count exceeds usize".to_string())?;
        let mut input_ring_sizes = Vec::with_capacity(input_count_usize);
        for index in 0..input_count_usize {
            input_ring_sizes.push(
                self.read_tx_input_summary()
                    .map_err(|e| format!("Failed to parse transaction input #{index}: {e}"))?,
            );
        }

        let output_count = self.read_varint()?;
        let output_count_usize: usize = output_count
            .try_into()
            .map_err(|_| "Transaction output count exceeds usize".to_string())?;
        for index in 0..output_count_usize {
            self.read_tx_output()
                .map_err(|e| format!("Failed to parse transaction output #{index}: {e}"))?;
        }

        let extra_len = self.read_varint()?;
        let extra_len_usize: usize = extra_len
            .try_into()
            .map_err(|_| "Transaction extra length exceeds usize".to_string())?;
        self.skip_bytes(extra_len_usize)?;

        if version == 1 {
            return Err(
                "Version 1 signed transactions are not supported by this parser".to_string(),
            );
        }

        let (rct_type, rct_fee) = if input_count_usize == 0 {
            (None, None)
        } else {
            let (rct_type, rct_fee) = self.read_rct_sig_base(input_count, output_count)?;
            if rct_type != 0 {
                let mixin = input_ring_sizes
                    .first()
                    .copied()
                    .and_then(|ring_size| ring_size.checked_sub(1))
                    .ok_or_else(|| "Transaction ring size is empty".to_string())?;
                self.read_rct_sig_prunable(rct_type, input_count, output_count, mixin)?;
            }
            (Some(rct_type), rct_fee)
        };

        let tx_blob = self.bytes[tx_start..self.offset].to_vec();
        let mut cursor = Cursor::new(tx_blob.as_slice());
        let tx = Transaction::read(&mut cursor)
            .map_err(|e| format!("Parsed signed txset transaction is not valid: {e:?}"))?;
        let mut trailing = [0u8; 1];
        if cursor
            .read(&mut trailing)
            .map_err(|e| format!("Failed to validate signed txset transaction cursor: {e:?}"))?
            != 0
        {
            return Err("Parsed signed txset transaction has trailing bytes".to_string());
        }
        let canonical_blob = tx.serialize();
        if canonical_blob != tx_blob {
            return Err("Parsed signed txset transaction is not canonical".to_string());
        }
        let tx_hash = tx.hash();

        Ok(TransactionSummary {
            hash: tx_hash,
            blob: tx_blob,
            version,
            unlock_time,
            input_count,
            input_ring_sizes,
            output_count,
            extra_len: extra_len_usize,
            rct_type,
            rct_fee,
        })
    }

    fn read_tx_input_summary(&mut self) -> Result<u64, String> {
        match self.read_u8()? {
            0xff => {
                let _height = self.read_varint()?;
                Ok(0)
            }
            0x00 => {
                self.skip_bytes(32)?; // prev
                let _prevout = self.read_varint()?;
                self.skip_vec_bytes()?; // sigset
                Ok(0)
            }
            0x01 => {
                self.skip_bytes(32)?; // prev
                let _prevout = self.read_varint()?;
                self.skip_txout_to_script()?;
                self.skip_vec_bytes()?; // sigset
                Ok(0)
            }
            0x02 => {
                let _amount = self.read_varint()?;
                let ring_size = self.read_varint()?;
                for _ in 0..ring_size {
                    let _ = self.read_varint()?;
                }
                self.skip_bytes(32)?; // key image
                Ok(ring_size)
            }
            tag => Err(format!(
                "Unsupported transaction input variant tag: 0x{tag:02x}"
            )),
        }
    }

    fn read_tx_output(&mut self) -> Result<(), String> {
        let _amount = self.read_varint()?;
        match self.read_u8()? {
            0x00 => self.skip_txout_to_script(),
            0x01 => self.skip_bytes(32),
            0x02 => self.skip_bytes(32),
            0x03 => self.skip_bytes(33),
            tag => Err(format!(
                "Unsupported transaction output variant tag: 0x{tag:02x}"
            )),
        }
    }

    fn skip_txout_to_script(&mut self) -> Result<(), String> {
        let key_count = self.read_varint()?;
        let key_count_usize: usize = key_count
            .try_into()
            .map_err(|_| "txout_to_script key count exceeds usize".to_string())?;
        self.skip_bytes(
            key_count_usize
                .checked_mul(32)
                .ok_or_else(|| "txout_to_script key byte count overflow".to_string())?,
        )?;
        self.skip_vec_bytes()
    }

    fn read_rct_sig_base(
        &mut self,
        inputs: u64,
        outputs: u64,
    ) -> Result<(u8, Option<u64>), String> {
        let rct_type = self.read_u8()?;
        if rct_type == 0 {
            return Ok((rct_type, None));
        }
        if !matches!(rct_type, 1..=6) {
            return Err(format!(
                "Unsupported RingCT type in signed txset: {rct_type}"
            ));
        }

        let fee = self.read_varint()?;

        if rct_type == 2 {
            self.skip_key_vector(inputs, "simple RingCT pseudoOuts")?;
        }

        for _ in 0..outputs {
            if matches!(rct_type, 4 | 5 | 6) {
                self.skip_bytes(8)?;
            } else {
                self.skip_bytes(64)?;
            }
        }

        self.skip_key_vector(outputs, "RingCT output masks")?;
        Ok((rct_type, Some(fee)))
    }

    fn read_rct_sig_prunable(
        &mut self,
        rct_type: u8,
        inputs: u64,
        _outputs: u64,
        mixin: u64,
    ) -> Result<(), String> {
        match rct_type {
            5 => {
                let bulletproof_count = self.read_varint()?;
                for index in 0..bulletproof_count {
                    self.skip_bulletproof()
                        .map_err(|e| format!("Failed to parse bulletproof #{index}: {e}"))?;
                }
                self.skip_clsags(inputs, mixin)?;
                self.skip_key_vector(inputs, "CLSAG pseudoOuts")?;
                Ok(())
            }
            6 => {
                let bulletproof_plus_count = self.read_varint()?;
                for index in 0..bulletproof_plus_count {
                    self.skip_bulletproof_plus()
                        .map_err(|e| format!("Failed to parse bulletproof+ #{index}: {e}"))?;
                }
                self.skip_clsags(inputs, mixin)?;
                self.skip_key_vector(inputs, "Bulletproof+ pseudoOuts")?;
                Ok(())
            }
            other => Err(format!(
                "Unsupported RingCT prunable type in signed txset: {other}"
            )),
        }
    }

    fn skip_bulletproof(&mut self) -> Result<(), String> {
        self.skip_bytes(6 * 32)?;
        self.skip_serialized_key_vec("bulletproof L")?;
        self.skip_serialized_key_vec("bulletproof R")?;
        self.skip_bytes(3 * 32)
    }

    fn skip_bulletproof_plus(&mut self) -> Result<(), String> {
        self.skip_bytes(6 * 32)?;
        self.skip_serialized_key_vec("bulletproof+ L")?;
        self.skip_serialized_key_vec("bulletproof+ R")
    }

    fn skip_clsags(&mut self, inputs: u64, mixin: u64) -> Result<(), String> {
        let scalar_count = mixin
            .checked_add(1)
            .ok_or_else(|| "CLSAG scalar count overflow".to_string())?;
        let scalar_count_usize: usize = scalar_count
            .try_into()
            .map_err(|_| "CLSAG scalar count exceeds usize".to_string())?;
        for index in 0..inputs {
            self.skip_bytes(
                scalar_count_usize
                    .checked_mul(32)
                    .ok_or_else(|| "CLSAG scalar byte count overflow".to_string())?,
            )
            .map_err(|e| format!("Failed to parse CLSAG #{index} scalars: {e}"))?;
            self.skip_bytes(32)?; // c1
            self.skip_bytes(32)?; // D
        }
        Ok(())
    }

    fn read_tx_construction_data_summary(&mut self) -> Result<TxConstructionDataSummary, String> {
        let source_count = self.read_varint()?;
        let source_count_usize: usize = source_count
            .try_into()
            .map_err(|_| "Source count exceeds usize".to_string())?;
        let mut source_ring_sizes = Vec::with_capacity(source_count_usize);
        let mut sources = Vec::with_capacity(source_count_usize);
        for index in 0..source_count_usize {
            let source = self
                .read_tx_source_entry_summary()
                .map_err(|e| format!("Failed to parse source #{index}: {e}"))?;
            source_ring_sizes.push(source.ring.len() as u64);
            sources.push(source);
        }

        let change = self.read_tx_destination_entry_summary()?;
        let change_amount = change.amount;
        let split_destination_count = self.read_varint()?;
        let split_destination_count_usize: usize = split_destination_count
            .try_into()
            .map_err(|_| "Split destination count exceeds usize".to_string())?;
        let mut split_destination_total_amount = 0u64;
        let mut split_destinations = Vec::with_capacity(split_destination_count_usize);
        for index in 0..split_destination_count_usize {
            let destination = self
                .read_tx_destination_entry_summary()
                .map_err(|e| format!("Failed to parse split destination #{index}: {e}"))?;
            split_destination_total_amount = split_destination_total_amount
                .checked_add(destination.amount)
                .ok_or_else(|| "Split destination total amount overflow".to_string())?;
            split_destinations.push(destination);
        }

        let selected_transfer_count = self.read_varint()?;
        let selected_transfer_count_usize: usize = selected_transfer_count
            .try_into()
            .map_err(|_| "Selected transfer count exceeds usize".to_string())?;
        let mut selected_transfer_indices = Vec::with_capacity(selected_transfer_count_usize);
        for _ in 0..selected_transfer_count_usize {
            selected_transfer_indices.push(self.read_varint()?);
        }

        let extra = self.read_var_bytes()?;
        let extra_len_usize = extra.len();

        let unlock_time = self.read_u64_le()?;
        let construction_flags = self.read_u8()?;
        let rct_config_version = self.read_varint()?;
        if rct_config_version != 0 {
            return Err(format!(
                "Unsupported RCT config version in unsigned txset: {rct_config_version}"
            ));
        }
        let rct_range_proof_type = self.read_varint()?;
        let rct_bp_version = self.read_varint()?;

        let destination_count = self.read_varint()?;
        let destination_count_usize: usize = destination_count
            .try_into()
            .map_err(|_| "Destination count exceeds usize".to_string())?;
        let mut destination_total_amount = 0u64;
        let mut destinations = Vec::with_capacity(destination_count_usize);
        for index in 0..destination_count_usize {
            let destination = self
                .read_tx_destination_entry_summary()
                .map_err(|e| format!("Failed to parse destination #{index}: {e}"))?;
            destination_total_amount = destination_total_amount
                .checked_add(destination.amount)
                .ok_or_else(|| "Destination total amount overflow".to_string())?;
            destinations.push(destination);
        }

        let subaddr_account = self.read_u32_le()?;
        let subaddr_count = self.read_varint()?;
        let subaddr_count_usize: usize = subaddr_count
            .try_into()
            .map_err(|_| "Subaddress index count exceeds usize".to_string())?;
        let mut subaddr_indices = Vec::with_capacity(subaddr_count_usize);
        for _ in 0..subaddr_count_usize {
            let index = self.read_varint()?;
            subaddr_indices.push(
                index
                    .try_into()
                    .map_err(|_| "Subaddress index exceeds u32".to_string())?,
            );
        }

        Ok(TxConstructionDataSummary {
            source_count,
            source_ring_sizes,
            sources,
            change_amount,
            change,
            split_destination_count,
            split_destination_total_amount,
            split_destinations,
            selected_transfer_count,
            selected_transfer_indices,
            extra_len: extra_len_usize,
            extra,
            unlock_time,
            construction_flags,
            use_rct: construction_flags & 0x01 != 0,
            use_view_tags: construction_flags & 0x02 != 0,
            rct_range_proof_type,
            rct_bp_version,
            destination_count,
            destination_total_amount,
            destinations,
            subaddr_account,
            subaddr_indices,
        })
    }

    fn read_tx_source_entry_summary(&mut self) -> Result<TxSourceEntrySummary, String> {
        let outputs_count = self.read_varint()?;
        let outputs_count_usize: usize = outputs_count
            .try_into()
            .map_err(|_| "Source output count exceeds usize".to_string())?;
        let mut ring = Vec::with_capacity(outputs_count_usize);
        for _ in 0..outputs_count {
            let pair_size = self.read_varint()?;
            if pair_size != 2 {
                return Err(format!("Unexpected source output pair size: {pair_size}"));
            }
            let global_output_index = self.read_varint()?;
            let output_public_key = self.read_fixed()?;
            let commitment = self.read_fixed()?;
            ring.push(TxSourceOutputSummary {
                global_output_index,
                output_public_key,
                commitment,
            });
        }

        let real_output = self.read_u64_le()?;
        if real_output >= outputs_count {
            return Err(format!(
                "Source real output index {real_output} outside ring of {outputs_count}"
            ));
        }
        let real_out_tx_key = self.read_fixed()?;

        let additional_key_count = self.read_varint()?;
        let additional_key_count_usize: usize = additional_key_count
            .try_into()
            .map_err(|_| "Source additional tx key count exceeds usize".to_string())?;
        let mut real_out_additional_tx_keys = Vec::with_capacity(additional_key_count_usize);
        for _ in 0..additional_key_count {
            real_out_additional_tx_keys.push(self.read_fixed()?);
        }

        let real_output_in_tx_index = self.read_u64_le()?;
        let amount = self.read_u64_le()?;
        let rct = self.read_bool()?;
        let mask = self.read_fixed()?;
        self.skip_bytes(128)?; // rct::multisig_kLRki

        Ok(TxSourceEntrySummary {
            ring,
            real_output,
            real_out_tx_key,
            real_out_additional_tx_keys,
            real_output_in_tx_index,
            amount,
            rct,
            mask,
        })
    }

    fn read_tx_destination_entry_summary(&mut self) -> Result<TxDestinationEntrySummary, String> {
        let original = self.read_var_bytes()?;
        let original_len = original.len();
        let amount = self.read_varint()?;
        let spend_public_key = self.read_fixed()?;
        let view_public_key = self.read_fixed()?;
        let is_subaddress = self.read_bool()?;
        let is_integrated = self.read_bool()?;
        Ok(TxDestinationEntrySummary {
            original_len,
            original,
            amount,
            spend_public_key,
            view_public_key,
            is_subaddress,
            is_integrated,
        })
    }

    fn read_exported_transfer_details(&mut self) -> Result<ExportedTransferDetails, String> {
        let version = self.read_varint()?;
        if version != 1 {
            return Err(format!(
                "Unsupported exported_transfer_details version: {version}"
            ));
        }

        let output_public_key = self.read_fixed()?;
        let internal_output_index = self.read_varint()?;
        let global_output_index = self.read_varint()?;
        let tx_public_key = self.read_fixed()?;
        let flags = ExportedTransferFlags::from_raw(self.read_u8()?);
        let amount = self.read_varint()?;
        let additional_count = self.read_varint()?;
        let additional_count: usize = additional_count
            .try_into()
            .map_err(|_| "Additional tx key count exceeds usize".to_string())?;
        let mut additional_tx_keys = Vec::with_capacity(additional_count);
        for _ in 0..additional_count {
            additional_tx_keys.push(self.read_fixed()?);
        }
        let subaddress_major = self.read_varint()?;
        let subaddress_minor = self.read_varint()?;

        Ok(ExportedTransferDetails {
            output_public_key,
            internal_output_index,
            global_output_index,
            tx_public_key,
            flags,
            amount,
            additional_tx_keys,
            subaddress_major,
            subaddress_minor,
        })
    }

    fn read_fixed<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let end = self
            .offset
            .checked_add(N)
            .ok_or_else(|| "Binary archive cursor overflow".to_string())?;
        if end > self.bytes.len() {
            return Err("Unexpected end of binary archive".to_string());
        }
        let mut out = [0u8; N];
        out.copy_from_slice(&self.bytes[self.offset..end]);
        self.offset = end;
        Ok(out)
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        Ok(self.read_fixed::<1>()?[0])
    }

    fn read_bool(&mut self) -> Result<bool, String> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(format!("Invalid binary archive bool value: {value}")),
        }
    }

    fn read_u32_le(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.read_fixed()?))
    }

    fn read_u64_le(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.read_fixed()?))
    }

    fn skip_bytes(&mut self, len: usize) -> Result<(), String> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| "Binary archive cursor overflow".to_string())?;
        if end > self.bytes.len() {
            return Err("Unexpected end of binary archive".to_string());
        }
        self.offset = end;
        Ok(())
    }

    fn read_var_bytes(&mut self) -> Result<Vec<u8>, String> {
        let len = self.read_varint()?;
        let len: usize = len
            .try_into()
            .map_err(|_| "Byte vector length exceeds usize".to_string())?;
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| "Binary archive cursor overflow".to_string())?;
        if end > self.bytes.len() {
            return Err("Unexpected end of binary archive".to_string());
        }
        let bytes = self.bytes[self.offset..end].to_vec();
        self.offset = end;
        Ok(bytes)
    }

    fn skip_vec_bytes(&mut self) -> Result<(), String> {
        let len = self.read_varint()?;
        let len: usize = len
            .try_into()
            .map_err(|_| "Byte vector length exceeds usize".to_string())?;
        self.skip_bytes(len)
    }

    fn skip_key_vector(&mut self, count: u64, label: &str) -> Result<(), String> {
        let count: usize = count
            .try_into()
            .map_err(|_| format!("{label} count exceeds usize"))?;
        self.skip_bytes(
            count
                .checked_mul(32)
                .ok_or_else(|| format!("{label} byte count overflow"))?,
        )
    }

    fn skip_serialized_key_vec(&mut self, label: &str) -> Result<(), String> {
        let count = self.read_varint()?;
        self.skip_key_vector(count, label)
    }

    fn read_varint(&mut self) -> Result<u64, String> {
        let mut bits = 0u32;
        let mut value = 0u64;
        loop {
            let byte = self.read_u8()?;
            if bits != 0 && byte == 0 {
                return Err("Non-canonical binary archive varint".to_string());
            }
            if bits >= 64 || ((bits + 7) > 64 && byte >= (1u8 << (64 - bits))) {
                return Err("Binary archive varint overflow".to_string());
            }

            value |= u64::from(byte & 0x7f) << bits;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
            bits += 7;
        }
    }
}

impl ExportedTransferFlags {
    fn from_raw(raw: u8) -> Self {
        Self {
            raw,
            spent: raw & 0x01 != 0,
            frozen: raw & 0x02 != 0,
            rct: raw & 0x04 != 0,
            key_image_known: raw & 0x08 != 0,
            key_image_request: raw & 0x10 != 0,
            key_image_partial: raw & 0x20 != 0,
        }
    }
}

#[deprecated(note = "Use key_image_signing::export_key_images_v3 for interop-compatible format")]
pub fn export_key_images(key_images: &[ExportedKeyImage]) -> Result<Vec<u8>, String> {
    #[derive(Serialize)]
    struct KeyImageExport {
        key_images: Vec<KeyImageEntry>,
    }

    #[derive(Serialize)]
    struct KeyImageEntry {
        key_image: Vec<u8>,
    }

    let entries: Result<Vec<_>, _> = key_images
        .iter()
        .map(|ki| {
            let bytes = hex::decode(&ki.key_image)
                .map_err(|e| format!("Invalid key image hex: {:?}", e))?;
            if bytes.len() != 32 {
                return Err("Key image must be 32 bytes".to_string());
            }
            Ok(KeyImageEntry { key_image: bytes })
        })
        .collect();

    let export = KeyImageExport {
        key_images: entries?,
    };
    let epee_data =
        monero_epee_bin_serde::to_bytes(&export).map_err(|e| format!("epee serialize: {:?}", e))?;

    Ok(wrap_with_magic(KEY_IMAGES_MAGIC, &epee_data))
}

/// Import key images from either old EPEE format or new v3 encrypted format.
///
/// - If `view_secret_key` is `None`, only old EPEE format is supported.
/// - If `view_secret_key` is `Some`, tries old EPEE format first, then v3.
pub fn import_key_images(
    data: &[u8],
    view_secret_key: Option<&[u8; 32]>,
) -> Result<Vec<String>, String> {
    let payload = strip_magic(KEY_IMAGES_MAGIC, data)?;

    // Try old EPEE format first.
    // Note: EPEE serializes Vec<u8> as a byte array, so we must use
    // serde_bytes::ByteBuf for deserialization to handle this correctly.
    #[derive(Deserialize)]
    struct KeyImageExport {
        key_images: Vec<KeyImageEntry>,
    }

    #[derive(Deserialize)]
    struct KeyImageEntry {
        #[serde(with = "serde_bytes")]
        key_image: Vec<u8>,
    }

    if let Ok(export) = monero_epee_bin_serde::from_bytes::<KeyImageExport, _>(payload) {
        return Ok(export
            .key_images
            .into_iter()
            .map(|e| hex::encode(e.key_image))
            .collect());
    }

    // EPEE failed; try v3 encrypted format if view key is provided
    match view_secret_key {
        Some(vsk) => {
            let (key_images, _pub_spend, _pub_view) =
                crate::key_image_signing::import_key_images_v3(data, vsk)?;
            Ok(key_images.iter().map(hex::encode).collect())
        }
        None => Err(
            "Data is not in old EPEE format and no view secret key provided for v3 decryption"
                .to_string(),
        ),
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;
    use wasm_bindgen_test::*;

    #[wasm_bindgen_test]
    fn epee_magic_wrap_strip_roundtrip_wasm() {
        let data = b"hello wasm";
        let wrapped = wrap_with_magic(UNSIGNED_TX_MAGIC, data);
        let stripped = strip_magic(UNSIGNED_TX_MAGIC, &wrapped).unwrap();
        assert_eq!(stripped, data);
    }

    #[wasm_bindgen_test]
    fn epee_magic_mismatch_wasm() {
        let wrapped = wrap_with_magic(UNSIGNED_TX_MAGIC, b"data");
        assert!(strip_magic(SIGNED_TX_MAGIC, &wrapped).is_err());
    }

    #[wasm_bindgen_test]
    fn epee_magic_too_short_wasm() {
        assert!(strip_magic(UNSIGNED_TX_MAGIC, b"x").is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_wrap_strip_roundtrip() {
        let data = b"hello world";
        let wrapped = wrap_with_magic(UNSIGNED_TX_MAGIC, data);
        let stripped = strip_magic(UNSIGNED_TX_MAGIC, &wrapped).unwrap();
        assert_eq!(stripped, data);
    }

    #[test]
    fn test_magic_mismatch() {
        let wrapped = wrap_with_magic(UNSIGNED_TX_MAGIC, b"data");
        let result = strip_magic(SIGNED_TX_MAGIC, &wrapped);
        assert!(result.is_err());
    }

    #[test]
    fn test_magic_too_short() {
        let result = strip_magic(UNSIGNED_TX_MAGIC, b"short");
        assert!(result.is_err());
    }

    #[test]
    fn test_detect_monero_txset() {
        assert_eq!(
            detect_monero_txset(b"Monero unsigned tx set\x05payload"),
            Some(MoneroTxSetKind::Unsigned)
        );
        assert_eq!(
            detect_monero_txset(b"Monero signed tx set\x05payload"),
            Some(MoneroTxSetKind::Signed)
        );
        assert_eq!(detect_monero_txset(b"other payload"), None);
    }

    #[test]
    fn test_inspect_monero_txset_decrypts_current_v5_payload() {
        let view_secret_key = [7u8; 32];
        let plaintext = [2u8, 1u8];
        let encrypted =
            crate::key_image_signing::encrypt_with_view_key(&plaintext, &view_secret_key);
        let txset = wrap_with_magic(UNSIGNED_TX_MAGIC, &encrypted);

        let inspection = inspect_monero_txset(&txset, &view_secret_key)
            .expect("synthetic wallet2 txset should inspect");

        assert_eq!(inspection.kind, MoneroTxSetKind::Unsigned);
        assert_eq!(inspection.encrypted_payload_len, encrypted.len());
        assert_eq!(inspection.decrypted_archive_len, plaintext.len());
        assert_eq!(inspection.archive_version, 2);
        assert_eq!(inspection.transaction_count, 1);
    }

    #[test]
    fn test_inspect_signed_monero_txset_prefix() {
        let view_secret_key = [7u8; 32];
        let plaintext = [0u8, 2u8];
        let encrypted =
            crate::key_image_signing::encrypt_with_view_key(&plaintext, &view_secret_key);
        let txset = wrap_with_magic(SIGNED_TX_MAGIC, &encrypted);

        let inspection = inspect_monero_txset(&txset, &view_secret_key)
            .expect("synthetic signed wallet2 txset should inspect");

        assert_eq!(inspection.kind, MoneroTxSetKind::Signed);
        assert_eq!(inspection.archive_version, 0);
        assert_eq!(inspection.transaction_count, 2);
    }

    #[test]
    fn test_inspect_monero_txset_rejects_wrong_archive_version() {
        let view_secret_key = [7u8; 32];
        let plaintext = [1u8, 1u8];
        let encrypted =
            crate::key_image_signing::encrypt_with_view_key(&plaintext, &view_secret_key);
        let txset = wrap_with_magic(UNSIGNED_TX_MAGIC, &encrypted);

        let err = inspect_monero_txset(&txset, &view_secret_key)
            .expect_err("wrong unsigned wallet2 archive version should fail");

        assert!(
            err.contains("Unsupported Monero unsigned tx set wallet2 archive version: 1"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_inspect_output_export_header() {
        let view_secret_key = [7u8; 32];
        let public_spend_key = [1u8; 32];
        let public_view_key = [2u8; 32];
        let archive_body = b"archive-body";
        let mut plaintext = Vec::new();
        plaintext.extend_from_slice(&public_spend_key);
        plaintext.extend_from_slice(&public_view_key);
        plaintext.extend_from_slice(archive_body);

        let encrypted =
            crate::key_image_signing::encrypt_with_view_key(&plaintext, &view_secret_key);
        let exported = wrap_with_magic(OUTPUT_EXPORT_MAGIC, &encrypted);
        let preview = inspect_output_export(&exported, &view_secret_key)
            .expect("synthetic output export should inspect");

        assert_eq!(preview.public_spend_key, public_spend_key);
        assert_eq!(preview.public_view_key, public_view_key);
        assert_eq!(preview.archive_body_len, archive_body.len());
    }

    #[test]
    fn test_parse_output_export_body() {
        fn write_varint(mut value: u64, out: &mut Vec<u8>) {
            loop {
                let mut byte = (value & 0x7f) as u8;
                value >>= 7;
                if value != 0 {
                    byte |= 0x80;
                }
                out.push(byte);
                if value == 0 {
                    break;
                }
            }
        }

        let view_secret_key = [7u8; 32];
        let public_spend_key = [1u8; 32];
        let public_view_key = [2u8; 32];
        let output_public_key = [3u8; 32];
        let tx_public_key = [4u8; 32];
        let additional_tx_key = [5u8; 32];

        let mut archive_body = Vec::new();
        write_varint(3, &mut archive_body); // tuple size
        write_varint(2, &mut archive_body); // offset
        write_varint(9, &mut archive_body); // total outputs
        write_varint(1, &mut archive_body); // output count
        write_varint(1, &mut archive_body); // exported_transfer_details version
        archive_body.extend_from_slice(&output_public_key);
        write_varint(6, &mut archive_body); // internal output index
        write_varint(12345, &mut archive_body); // global output index
        archive_body.extend_from_slice(&tx_public_key);
        archive_body.push(0x1d); // spent, rct, key_image_known, key_image_request
        write_varint(1_000_000_000_000, &mut archive_body);
        write_varint(1, &mut archive_body); // additional tx key count
        archive_body.extend_from_slice(&additional_tx_key);
        write_varint(3, &mut archive_body); // subaddress major
        write_varint(4, &mut archive_body); // subaddress minor

        let mut plaintext = Vec::new();
        plaintext.extend_from_slice(&public_spend_key);
        plaintext.extend_from_slice(&public_view_key);
        plaintext.extend_from_slice(&archive_body);

        let encrypted =
            crate::key_image_signing::encrypt_with_view_key(&plaintext, &view_secret_key);
        let exported = wrap_with_magic(OUTPUT_EXPORT_MAGIC, &encrypted);
        let parsed = parse_output_export(&exported, &view_secret_key)
            .expect("synthetic output export should parse");

        assert_eq!(parsed.public_spend_key, public_spend_key);
        assert_eq!(parsed.public_view_key, public_view_key);
        assert_eq!(parsed.offset, 2);
        assert_eq!(parsed.total_outputs, 9);
        assert_eq!(parsed.outputs.len(), 1);

        let output = &parsed.outputs[0];
        assert_eq!(output.output_public_key, output_public_key);
        assert_eq!(output.internal_output_index, 6);
        assert_eq!(output.global_output_index, 12345);
        assert_eq!(output.tx_public_key, tx_public_key);
        assert_eq!(output.amount, 1_000_000_000_000);
        assert_eq!(output.additional_tx_keys, vec![additional_tx_key]);
        assert_eq!(output.subaddress_major, 3);
        assert_eq!(output.subaddress_minor, 4);
        assert_eq!(output.flags.raw, 0x1d);
        assert!(output.flags.spent);
        assert!(!output.flags.frozen);
        assert!(output.flags.rct);
        assert!(output.flags.key_image_known);
        assert!(output.flags.key_image_request);
        assert!(!output.flags.key_image_partial);
    }

    #[test]
    fn test_backward_compat_import_old_epee_format() {
        // Create data in the old EPEE format
        let key_images = vec![ExportedKeyImage {
            key_image: "a".repeat(64), // 32 bytes as hex
            tx_hash: "b".repeat(64),
            output_index: 0,
        }];

        #[allow(deprecated)]
        let exported = export_key_images(&key_images).expect("old export should work");

        // Import with None view key (old format only)
        let imported = import_key_images(&exported, None).expect("old import should work");
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0], "a".repeat(64));
    }

    #[test]
    fn test_import_rejects_non_epee_without_view_key() {
        // Construct data that looks like it has magic but isn't EPEE
        let mut data = Vec::new();
        data.extend_from_slice(KEY_IMAGES_MAGIC);
        data.extend_from_slice(b"this is not epee data at all!");

        let result = import_key_images(&data, None);
        assert!(
            result.is_err(),
            "Non-EPEE data without view key should fail"
        );
    }
}
