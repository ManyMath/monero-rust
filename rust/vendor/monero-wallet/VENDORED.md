# Vendored monero-wallet

This is `monero-wallet` 0.2.0 as published on crates.io (checksum
`b8d9a2431255db4deaf39671646e17f79bfb4ceadfffc9bb52e0f3fb0d29c75e`, built from
monero-oxide commit `9e11f5c0f2b18ab821192efa427c863086b51379`, path
`monero-oxide/wallet`).

Packaging differences from the published crate:

- `Cargo.toml`: the integration test targets and dev-dependencies are removed.
- `tests/`, `Cargo.lock`, `Cargo.toml.orig` and `.cargo_vcs_info.json` are not copied.
- `rustfmt.toml` is added to keep upstream's style.

Source changes, oldest first:

- `src/send/tx_keys.rs`: transaction keys follow wallet2's `classify_addresses`.
- `src/send/tx.rs`, `src/send/tx_keys.rs`, `src/send/mod.rs`: the payment ID is
  encrypted to the output wallet2's `get_destination_view_key_pub` chooses.
