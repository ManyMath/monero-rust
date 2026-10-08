//! Helpers that write and seal wallet2 unsigned txsets, as a hostile online
//! wallet can.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use monero_rust::epee_compat::{
    self, ExportedTransferDetails, ParsedUnsignedTxSetSummary, TxConstructionDataSummary,
    TxDestinationEntrySummary, TxSourceEntrySummary,
};
use monero_rust::key_image_signing::{decrypt_with_view_key, encrypt_with_view_key};
use monero_rust::wallet_keys_file::{read_keys_file, ImportedKeysFile};

pub fn vector_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/vectors/cold_signing_regtest_v0_18_5_0")
        .join(name)
}

pub fn cold_keys() -> ImportedKeysFile {
    read_keys_file(&vector_path("cold_full.keys"), "").expect("cold keys")
}

pub fn hot_keys() -> ImportedKeysFile {
    read_keys_file(&vector_path("hot_view_only.keys"), "").expect("hot keys")
}

pub fn vector_unsigned_txset() -> Vec<u8> {
    std::fs::read(vector_path("unsigned_monero_tx")).expect("unsigned txset vector")
}

pub fn varint(mut value: u64, out: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            return;
        }
    }
}

fn var_bytes(bytes: &[u8], out: &mut Vec<u8>) {
    varint(bytes.len() as u64, out);
    out.extend_from_slice(bytes);
}

pub fn write_destination(d: &TxDestinationEntrySummary, out: &mut Vec<u8>) {
    var_bytes(&d.original, out);
    varint(d.amount, out);
    out.extend_from_slice(&d.spend_public_key);
    out.extend_from_slice(&d.view_public_key);
    out.push(u8::from(d.is_subaddress));
    out.push(u8::from(d.is_integrated));
}

pub fn write_source(s: &TxSourceEntrySummary, out: &mut Vec<u8>) {
    varint(s.ring.len() as u64, out);
    for member in &s.ring {
        varint(2, out);
        varint(member.global_output_index, out);
        out.extend_from_slice(&member.output_public_key);
        out.extend_from_slice(&member.commitment);
    }
    out.extend_from_slice(&s.real_output.to_le_bytes());
    out.extend_from_slice(&s.real_out_tx_key);
    varint(s.real_out_additional_tx_keys.len() as u64, out);
    for key in &s.real_out_additional_tx_keys {
        out.extend_from_slice(key);
    }
    out.extend_from_slice(&s.real_output_in_tx_index.to_le_bytes());
    out.extend_from_slice(&s.amount.to_le_bytes());
    out.push(u8::from(s.rct));
    out.extend_from_slice(&s.mask);
    out.extend_from_slice(&[0u8; 128]); // rct::multisig_kLRki
}

pub fn write_construction(c: &TxConstructionDataSummary, out: &mut Vec<u8>) {
    varint(c.sources.len() as u64, out);
    for source in &c.sources {
        write_source(source, out);
    }
    write_destination(&c.change, out);
    varint(c.split_destinations.len() as u64, out);
    for d in &c.split_destinations {
        write_destination(d, out);
    }
    varint(c.selected_transfer_indices.len() as u64, out);
    for index in &c.selected_transfer_indices {
        varint(*index, out);
    }
    var_bytes(&c.extra, out);
    out.extend_from_slice(&c.unlock_time.to_le_bytes());
    out.push(c.construction_flags);
    varint(0, out); // rct_config version
    varint(c.rct_range_proof_type, out);
    varint(c.rct_bp_version, out);
    varint(c.destinations.len() as u64, out);
    for d in &c.destinations {
        write_destination(d, out);
    }
    out.extend_from_slice(&c.subaddr_account.to_le_bytes());
    varint(c.subaddr_indices.len() as u64, out);
    for index in &c.subaddr_indices {
        varint(u64::from(*index), out);
    }
}

pub fn write_exported_transfer(t: &ExportedTransferDetails, out: &mut Vec<u8>) {
    varint(1, out);
    out.extend_from_slice(&t.output_public_key);
    varint(t.internal_output_index, out);
    varint(t.global_output_index, out);
    out.extend_from_slice(&t.tx_public_key);
    out.push(t.flags.raw);
    varint(t.amount, out);
    varint(t.additional_tx_keys.len() as u64, out);
    for key in &t.additional_tx_keys {
        out.extend_from_slice(key);
    }
    varint(t.subaddress_major, out);
    varint(t.subaddress_minor, out);
}

pub fn unsigned_txset_archive(set: &ParsedUnsignedTxSetSummary) -> Vec<u8> {
    let mut out = Vec::new();
    varint(set.archive_version, &mut out);
    varint(set.txes.len() as u64, &mut out);
    for tx in &set.txes {
        write_construction(tx, &mut out);
    }
    varint(3, &mut out); // tuple size
    varint(set.new_transfer_first, &mut out);
    varint(set.new_transfer_second, &mut out);
    varint(set.new_transfers.len() as u64, &mut out);
    for transfer in &set.new_transfers {
        write_exported_transfer(transfer, &mut out);
    }
    out
}

pub fn seal(magic: &[u8], plaintext: &[u8], view_secret: &[u8; 32]) -> Vec<u8> {
    epee_compat::wrap_with_magic(magic, &encrypt_with_view_key(plaintext, view_secret))
}

pub fn seal_unsigned_txset(set: &ParsedUnsignedTxSetSummary, view_secret: &[u8; 32]) -> Vec<u8> {
    seal(
        epee_compat::UNSIGNED_TX_MAGIC,
        &unsigned_txset_archive(set),
        view_secret,
    )
}

pub fn open(magic: &[u8], data: &[u8], view_secret: &[u8; 32]) -> Vec<u8> {
    decrypt_with_view_key(
        epee_compat::strip_magic(magic, data).expect("magic"),
        view_secret,
    )
    .expect("decrypt")
}

pub fn parse_vector_unsigned() -> ParsedUnsignedTxSetSummary {
    let hot = hot_keys();
    epee_compat::parse_unsigned_monero_txset_summary(&vector_unsigned_txset(), &hot.view_secret_key)
        .expect("vector unsigned txset parses")
}

pub struct Xs(pub u64);

impl Xs {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

pub fn mutate(rng: &mut Xs, data: &mut Vec<u8>) {
    if data.is_empty() {
        data.push(rng.next() as u8);
        return;
    }
    match rng.below(8) {
        0 => {
            let i = rng.below(data.len());
            data[i] ^= 1 << rng.below(8);
        }
        1 => {
            let i = rng.below(data.len());
            data[i] = rng.next() as u8;
        }
        2 => {
            let i = rng.below(data.len());
            data.truncate(i);
        }
        3 => {
            let i = rng.below(data.len() + 1);
            data.insert(i, rng.next() as u8);
        }
        4 => {
            let start = rng.below(data.len());
            let len = rng.below((data.len() - start).min(64)) + 1;
            let span = data[start..(start + len).min(data.len())].to_vec();
            let at = rng.below(data.len() + 1);
            for (k, b) in span.into_iter().enumerate() {
                data.insert(at + k, b);
            }
        }
        5 => {
            let i = rng.below(data.len());
            let interesting = [0x00u8, 0x01, 0x7f, 0x80, 0xfe, 0xff];
            data[i] = interesting[rng.below(interesting.len())];
        }
        6 => {
            let i = rng.below(data.len());
            let len = rng.below(10) + 1;
            for k in 0..len {
                if i + k < data.len() {
                    data[i + k] = 0xff;
                }
            }
        }
        _ => {
            let i = rng.below(data.len());
            let len = rng.below((data.len() - i).min(32)) + 1;
            data.drain(i..(i + len).min(data.len()));
        }
    }
}
