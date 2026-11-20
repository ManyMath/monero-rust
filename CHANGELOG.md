# Changelog

## 0.0.5 - 2026-11-19

### Added

- Added priority-aware fee policies and RPC-backed preparation APIs for sends
  and sweeps.
- Added a durable ring database interface that records rings and decoy candidate
  sets across transaction rebuilds, restarts, imports, and reorgs.
- Added wallet refresh and mempool state types, pre-RingCT output reporting,
  pruned-block scanning, and explicit handling for deeper reorgs.
- Added checks for unsigned and signed wallet2 transaction sets and for
  transactions returned by an offline signer before relay.
- Added support for wallet2 ASCII-armored exports and keys files with configured
  KDF rounds.
- Added hostile-node, restricted-node, cold-signing, ring-reuse, fee, input
  selection, regtest, stagenet, and statistical comparison harnesses.

### Changed

- Replaced the `wallet_compat` types with `monero-wallet` outputs, decoys, view
  pairs, and fee-rate types, and moved fee estimation into its own module.
- Vendored `monero-wallet` 0.2.0 with recorded patches for address
  classification, payment IDs, and scanning version 1 transactions.
- Changed transaction preparation to expose fee and change bounds, use checked
  amount arithmetic, reject zero-value destinations, and split or stop adding
  inputs when a transaction reaches its configured weight policy.
- Changed sweep preparation to support explicit subaddress selection and
  multiple transactions.
- Changed scanning to persist global output indexes and spend heights, validate
  batch ancestry and output indexes, track pool outputs and spends, retain
  duplicate-key recovery data, and store block IDs as bytes.
- Changed daemon RPC handling to bound response sizes and call durations, slice
  large transaction and key-image requests, distinguish refused or missing
  transactions, and use wallet-specific request headers and client IDs.
- Changed transaction construction to reuse recorded rings, honor local output
  blackballs, persist ring data before export or relay, and check relayed rings
  against the ring database.
- Changed fee handling to support priorities, bound fee rates and quantization
  masks, use conservative transaction-weight bounds, and settle automatic
  priority once per transaction.
- Changed the native unsigned transaction format so it does not carry the view
  key or a preselected change key, binds change to the source account, bounds
  input counts, and mixes signer entropy into transaction seeds.
- Changed keys-file and signing code to redact and zeroize more secret material,
  use fresh encryption IVs, and require canonical signature scalars.
- Removed vendored Polyseed support. Sixteen-word seed imports now return an
  unsupported-Polyseed error.

### Fixed

- Fixed output selection across accounts, unlock-height calculation, integrated
  address handling, payment-ID placement, change scanning, and recipient fee
  subtraction.
- Fixed scan rollback, pool cleanup, frozen-output balances, zero-amount output
  handling, duplicate one-time keys, change payment IDs, and multi-wallet chain
  validation.
- Fixed parsing and validation of malformed binary RPC replies, wallet2 files,
  UR frames, key-image imports, and offline signed transactions.
