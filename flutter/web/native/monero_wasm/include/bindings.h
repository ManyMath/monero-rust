#ifndef MONERO_WASM_BINDINGS_H
#define MONERO_WASM_BINDINGS_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * Numeric signal IDs shared between Rust and Dart.
 * DartSignal = Dart->Rust (requests), RustSignal = Rust->Dart (responses).
 *
 * IMPORTANT: These IDs must stay in sync with `hub_signal_ids.dart` on the Dart side.
 */
#define MONERO_TEST_REQUEST 1

#define CREATE_WALLET_REQUEST 2

#define START_SYNC_REQUEST 3

#define GET_BALANCE_REQUEST 4

#define CREATE_TRANSACTION_REQUEST 5

#define SWEEP_ALL_REQUEST 6

#define GENERATE_SEED_REQUEST 7

#define GET_SEED_BIRTHDAY_REQUEST 8

#define GET_BLOCK_HEIGHT_FROM_TIMESTAMP_REQUEST 9

#define DERIVE_ADDRESS_REQUEST 10

#define DERIVE_SUBADDRESS_REQUEST 11

#define DERIVE_KEYS_REQUEST 12

#define SCAN_BLOCK_REQUEST 13

#define BROADCAST_TRANSACTION_REQUEST 14

#define QUERY_DAEMON_HEIGHT_REQUEST 15

#define START_CONTINUOUS_SCAN_REQUEST 16

#define STOP_SCAN_REQUEST 17

#define MEMPOOL_SCAN_REQUEST 18

#define GENERATE_OUT_PROOF_REQUEST 19

#define SAVE_WALLET_DATA_REQUEST 20

#define LOAD_WALLET_DATA_REQUEST 21

#define DERIVE_ENCRYPTION_KEY_REQUEST 22

#define SAVE_WITH_DERIVED_KEY_REQUEST 23

#define SCAN_BLOCK_MULTI_WALLET_REQUEST 24

#define START_MULTI_WALLET_SCAN_REQUEST 25

#define RESTORE_WALLET_DATA_REQUEST 26

#define GET_BLOCK_HASHES_REQUEST 27

#define GET_PENDING_STATE_REQUEST 28

#define CONVERT_BIP39_TO_LEGACY_REQUEST 29

#define FREEZE_OUTPUT_REQUEST 30

#define THAW_OUTPUT_REQUEST 31

#define CREATE_UNSIGNED_TRANSACTION_REQUEST 32

#define SIGN_UNSIGNED_TRANSACTION_REQUEST 33

#define EXPORT_KEY_IMAGES_REQUEST 34

#define IMPORT_KEY_IMAGES_REQUEST 35

#define MONERO_TEST_RESPONSE 101

#define WALLET_CREATED_RESPONSE 102

#define SYNC_PROGRESS_RESPONSE 103

#define BALANCE_RESPONSE 104

#define TRANSACTION_CREATED_RESPONSE 105

#define SEED_GENERATED_RESPONSE 106

#define SEED_BIRTHDAY_RESPONSE 107

#define BLOCK_HEIGHT_FROM_TIMESTAMP_RESPONSE 108

#define ADDRESS_DERIVED_RESPONSE 109

#define SUBADDRESS_DERIVED_RESPONSE 110

#define KEYS_DERIVED_RESPONSE 111

#define BLOCK_SCAN_RESPONSE 112

#define TRANSACTION_BROADCAST_RESPONSE 113

#define DAEMON_HEIGHT_RESPONSE 114

#define SPENT_STATUS_UPDATED_RESPONSE 115

#define MEMPOOL_SCAN_RESPONSE 116

#define OUT_PROOF_GENERATED_RESPONSE 117

#define WALLET_DATA_SAVED_RESPONSE 118

#define WALLET_DATA_LOADED_RESPONSE 119

#define ENCRYPTION_KEY_DERIVED_RESPONSE 120

#define MULTI_WALLET_SCAN_RESPONSE 121

#define BLOCK_HASHES_RESPONSE 122

#define PENDING_STATE_RESPONSE 123

#define REORG_DETECTED_RESPONSE 124

#define DOUBLE_SPEND_DETECTED_RESPONSE 125

#define BIP39_LEGACY_SEED_RESPONSE 126

#define FREEZE_THAW_RESPONSE 127

#define TRANSACTION_STATUS_UPDATE 128

#define UNSIGNED_TRANSACTION_CREATED_RESPONSE 129

#define TRANSACTION_SIGNED_OFFLINE_RESPONSE 130

#define KEY_IMAGES_EXPORTED_RESPONSE 131

#define KEY_IMAGES_IMPORTED_RESPONSE 132

/**
 * Opaque handle returned by [`hub_init`]. Holds the tokio runtime.
 */
typedef struct HubHandle HubHandle;

/**
 * Function pointer type for Rust -> Dart signal delivery.
 *
 * # Safety contract (caller of `hub_init` must guarantee)
 *
 * * `callback` must be safe to invoke from any thread
 * * `user_data` must remain valid until [`hub_shutdown`] is called
 */
typedef void (*RustSignalCallback)(uint32_t signal_id,
                                   const uint8_t *data_ptr,
                                   uintptr_t data_len,
                                   void *user_data);

/**
 * Initialize the hub runtime and register the Dart callback.
 *
 * Returns an opaque [`HubHandle`] pointer that must be passed to
 * [`hub_send_dart_signal`] and eventually freed with [`hub_shutdown`].
 * Returns null on failure.
 */
struct HubHandle *hub_init(RustSignalCallback callback, void *user_data);

/**
 * Send a DartSignal (Dart -> Rust) into the hub runtime.
 *
 * # Safety
 *
 * `handle` must be a valid pointer returned by `hub_init`.
 * `data_ptr` must point to `data_len` readable bytes, or be non-null
 * with `data_len == 0`.
 */
void hub_send_dart_signal(struct HubHandle *handle,
                          uint32_t signal_id,
                          const uint8_t *data_ptr,
                          uintptr_t data_len);

/**
 * Shut down the hub runtime and free the handle.
 *
 * # Safety
 *
 * `handle` must be a valid pointer returned by `hub_init`, and must not
 * be used after this call.
 */
void hub_shutdown(struct HubHandle *handle);

/**
 * Free a byte buffer that was allocated by Rust via [`alloc_rust_bytes`]
 * and passed to Dart.
 *
 * # Safety
 *
 * `ptr` must have been allocated by [`alloc_rust_bytes`] (which uses
 * `Box<[u8]>`), and `len` must match the original allocation length.
 */
void hub_free_bytes(uint8_t *ptr, uintptr_t len);

#endif  /* MONERO_WASM_BINDINGS_H */
