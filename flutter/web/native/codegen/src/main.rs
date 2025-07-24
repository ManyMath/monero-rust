mod dart_gen;
mod model;
mod parser;
mod rust_gen;

use std::path::PathBuf;

const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let check_mode = std::env::args().any(|a| a == "--check");

    let manifest = PathBuf::from(MANIFEST_DIR);

    // Input: signals/mod.rs
    let signals_path = manifest.join("../monero_wasm/src/signals/mod.rs");

    // Outputs
    let signal_types_path = manifest.join("../../lib/src/ffi/signal_types.dart");
    let ffi_web_path = manifest.join("../monero_wasm/src/ffi_web.rs");
    let signal_ids_path = manifest.join("../monero_wasm/src/signal_ids.rs");
    let hub_signal_ids_path = manifest.join("../../lib/src/transport/hub_signal_ids.dart");

    // Parse
    let src = std::fs::read_to_string(&signals_path).map_err(|e| {
        format!("failed to read {}: {}", signals_path.display(), e)
    })?;
    let signals = parser::parse_signals(&src)?;

    eprintln!(
        "Parsed {} signals ({} Dart2Rust, {} Rust2Dart, {} Shared)",
        signals.len(),
        signals.iter().filter(|s| s.direction == model::Direction::Dart2Rust).count(),
        signals.iter().filter(|s| s.direction == model::Direction::Rust2Dart).count(),
        signals.iter().filter(|s| s.direction == model::Direction::Shared).count(),
    );

    // Generate
    let signal_types = dart_gen::generate(&signals);
    let ffi_web = rust_gen::generate_ffi_web(&signals);
    let signal_ids = rust_gen::generate_signal_ids(&signals);
    let hub_signal_ids = rust_gen::generate_hub_signal_ids(&signals);

    let outputs: Vec<(&PathBuf, &str)> = vec![
        (&signal_types_path, &signal_types),
        (&ffi_web_path, &ffi_web),
        (&signal_ids_path, &signal_ids),
        (&hub_signal_ids_path, &hub_signal_ids),
    ];

    if check_mode {
        let mut has_diff = false;
        for (path, generated) in &outputs {
            let existing = std::fs::read_to_string(path).unwrap_or_default();
            if existing != *generated {
                eprintln!("DIFF: {}", path.display());
                print_diff(&existing, generated);
                has_diff = true;
            } else {
                eprintln!("OK:   {}", path.display());
            }
        }
        if has_diff {
            std::process::exit(1);
        }
    } else {
        for (path, generated) in &outputs {
            std::fs::write(path, generated).map_err(|e| {
                format!("failed to write {}: {}", path.display(), e)
            })?;
            eprintln!("Wrote {}", path.display());
        }
    }

    Ok(())
}

/// Print a simple line-diff to stderr
fn print_diff(existing: &str, generated: &str) {
    let ex_lines: Vec<&str> = existing.lines().collect();
    let gen_lines: Vec<&str> = generated.lines().collect();

    let max = ex_lines.len().max(gen_lines.len());
    let mut shown = 0;
    for i in 0..max {
        let a = ex_lines.get(i).copied().unwrap_or("<missing>");
        let b = gen_lines.get(i).copied().unwrap_or("<missing>");
        if a != b {
            if shown == 0 {
                eprintln!("  --- existing / +++ generated");
            }
            eprintln!("  line {}: - {}", i + 1, a);
            eprintln!("  line {}: + {}", i + 1, b);
            shown += 1;
            if shown >= 20 {
                eprintln!("  ... (truncated, {} more lines differ)", max - i - 1);
                break;
            }
        }
    }
}
