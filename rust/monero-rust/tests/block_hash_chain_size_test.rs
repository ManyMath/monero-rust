use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use monero_rust::{BlockHashChain, BlockId};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::SeqCst);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(new_size, Ordering::SeqCst);
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn retained<T>(make: impl FnOnce() -> T) -> (T, usize) {
    let before = LIVE.load(Ordering::SeqCst);
    let value = make();
    (value, LIVE.load(Ordering::SeqCst).saturating_sub(before))
}

fn id(height: u64) -> BlockId {
    let mut id = [0xa5u8; 32];
    id[..8].copy_from_slice(&height.to_le_bytes());
    id
}

#[derive(serde::Serialize)]
struct HexMapChain {
    hashes: BTreeMap<u64, String>,
    genesis_hash: Option<String>,
}

const CHECKPOINT: u64 = 550_000;

#[test]
fn block_ids_take_32_bytes_each() {
    let count: u64 = std::env::var("BLOCK_IDS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(200_000);
    let mb = |bytes: usize| bytes as f64 / 1e6;
    let per_id = |bytes: usize| bytes as f64 / (count + 1) as f64;

    let (old, old_memory) = retained(|| {
        let mut hashes = BTreeMap::new();
        hashes.insert(0, hex::encode(id(0)));
        for height in CHECKPOINT..CHECKPOINT + count {
            hashes.insert(height, hex::encode(id(height)));
        }
        HexMapChain {
            genesis_hash: Some(hex::encode(id(0))),
            hashes,
        }
    });
    let old_json = serde_json::to_vec(&old).unwrap();
    drop(old);

    let (recorded, recorded_memory) = retained(|| {
        let mut chain = BlockHashChain::new();
        chain.record_block(0, id(0));
        for height in CHECKPOINT..CHECKPOINT + count {
            chain.record_block(height, id(height));
        }
        chain
    });
    let json = serde_json::to_vec(&recorded).unwrap();
    let started = std::time::Instant::now();
    let (loaded, loaded_memory) =
        retained(|| serde_json::from_slice::<BlockHashChain>(&json).unwrap());
    let load_time = started.elapsed();
    let started = std::time::Instant::now();
    let from_old = serde_json::from_slice::<BlockHashChain>(&old_json).unwrap();
    let old_load_time = started.elapsed();
    assert_eq!(loaded, recorded);
    assert_eq!(from_old, recorded);
    assert_eq!(loaded.len() as u64, count + 1);
    assert_eq!(
        loaded.get_hash(CHECKPOINT + count - 1),
        Some(&id(CHECKPOINT + count - 1))
    );

    println!(
        "{} block ids (genesis and {count} from {CHECKPOINT}):",
        count + 1
    );
    println!(
        "  hex map (before): {:.1} MB in memory ({:.0} B/id), {:.1} MB of JSON ({:.0} B/id)",
        mb(old_memory),
        per_id(old_memory),
        mb(old_json.len()),
        per_id(old_json.len()),
    );
    println!(
        "  bytes (after): {:.1} MB in memory recorded one by one ({:.0} B/id), \
         {:.1} MB loaded ({:.0} B/id), {:.1} MB of JSON ({:.0} B/id)",
        mb(recorded_memory),
        per_id(recorded_memory),
        mb(loaded_memory),
        per_id(loaded_memory),
        mb(json.len()),
        per_id(json.len()),
    );
    println!("  load: {load_time:?} from bytes, {old_load_time:?} from the hex map");

    assert!(per_id(loaded_memory) < 33.0, "{loaded_memory}");
    assert!(per_id(recorded_memory) < 65.0, "{recorded_memory}");
    assert!(per_id(json.len()) < 43.0, "{}", json.len());
}
