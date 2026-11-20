//! Bounded allocation regression: hostile counts must not reserve record arrays
//! before parsing them. This isolated test binary measures the public encrypted
//! import path, including authentication and decryption.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use monero_rust::{epee_compat as e, key_image_signing as k};

struct TrackingAllocator;
static MAX_ALLOCATION: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        MAX_ALLOCATION.fetch_max(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        MAX_ALLOCATION.fetch_max(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        MAX_ALLOCATION.fetch_max(new_size, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;

fn varint(mut value: u64, bytes: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 127) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 128;
        }
        bytes.push(byte);
        if value == 0 {
            return;
        }
    }
}

#[test]
fn archive_allocations_follow_validated_records() {
    let key = [1u8; 32];
    // A 64 KiB body claiming u64::MAX transactions must not reserve storage
    // for the claimed records before any transaction has been validated.
    let mut malformed = vec![2];
    varint(u64::MAX, &mut malformed);
    malformed.resize(65536, 0);
    let encrypted = e::wrap_with_magic(
        e::UNSIGNED_TX_MAGIC,
        &k::encrypt_with_view_key(&malformed, &key),
    );
    MAX_ALLOCATION.store(0, Ordering::Relaxed);
    k::decrypt_with_view_key(&encrypted[e::UNSIGNED_TX_MAGIC.len()..], &key).unwrap();
    // CryptoNight's fixed scratchpad is independent of the archive count.
    let decryption_peak = MAX_ALLOCATION.load(Ordering::Relaxed);
    MAX_ALLOCATION.store(0, Ordering::Relaxed);
    assert!(e::parse_unsigned_monero_txset_summary(&encrypted, &key).is_err());
    let peak = MAX_ALLOCATION.load(Ordering::Relaxed);
    assert!(
        peak <= decryption_peak.max(encrypted.len() * 4),
        "{} input bytes caused {peak} allocation bytes",
        encrypted.len()
    );
    println!(
        "malformed archive: input={} maximum allocation={peak}",
        encrypted.len()
    );

    // A sizeable valid output archive remains supported. This exercises repeated
    // fallible growth and nested additional-key vectors, without a small count cap.
    let count = 4096u64;
    let mut plaintext = vec![0u8; 64]; // export account public keys
    varint(3, &mut plaintext); // tuple size
    varint(0, &mut plaintext); // offset
    varint(count, &mut plaintext); // total outputs
    varint(count, &mut plaintext);
    for index in 0..count {
        varint(1, &mut plaintext); // transfer version
        plaintext.extend_from_slice(&[3u8; 32]); // output key
        varint(0, &mut plaintext); // internal index
        varint(index, &mut plaintext); // global index
        plaintext.extend_from_slice(&[4u8; 32]); // tx key
        plaintext.push(0); // flags
        varint(1, &mut plaintext); // amount
        varint(1, &mut plaintext); // additional keys
        plaintext.extend_from_slice(&[5u8; 32]);
        varint(0, &mut plaintext); // subaddress major
        varint(0, &mut plaintext); // subaddress minor
    }
    let encrypted = e::wrap_with_magic(
        e::OUTPUT_EXPORT_MAGIC,
        &k::encrypt_with_view_key(&plaintext, &key),
    );
    let parsed = e::parse_output_export(&encrypted, &key).unwrap();
    assert_eq!(parsed.outputs.len(), count as usize);
    assert_eq!(
        parsed.outputs.last().unwrap().global_output_index,
        count - 1
    );
    assert_eq!(parsed.outputs[0].additional_tx_keys, vec![[5u8; 32]]);
}
