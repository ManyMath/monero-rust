// Auto-generated signal types for monero-rust FFI.
// Replaces rinf-generated bincode-based signal classes with JSON-based ones.

import 'dart:async';
import 'dart:convert';
import 'web_ffi.dart' as ffi;

// ===========================================================================
// Shared / sub-struct types (both toJson and fromJson)
// ===========================================================================

class Recipient {
  final String address;
  final int amount;

  Recipient({required this.address, required this.amount});

  factory Recipient.fromJson(Map<String, dynamic> json) {
    return Recipient(
      address: json['address'] as String,
      amount: json['amount'] as int,
    );
  }

  Map<String, dynamic> toJson() => {
        'address': address,
        'amount': amount,
      };
}

class OwnedOutput {
  final String txHash;
  final int outputIndex;
  final int amount;
  final String amountXmr;
  final String key;
  final String keyOffset;
  final String commitmentMask;
  final List<int>? subaddressIndex;
  final String? paymentId;
  final String receivedOutputBytes;
  final int blockHeight;
  final bool spent;
  final String keyImage;
  final bool isCoinbase;
  final bool frozen;

  OwnedOutput({
    required this.txHash,
    required this.outputIndex,
    required this.amount,
    required this.amountXmr,
    required this.key,
    required this.keyOffset,
    required this.commitmentMask,
    this.subaddressIndex,
    this.paymentId,
    required this.receivedOutputBytes,
    required this.blockHeight,
    required this.spent,
    required this.keyImage,
    required this.isCoinbase,
    required this.frozen,
  });

  factory OwnedOutput.fromJson(Map<String, dynamic> json) {
    return OwnedOutput(
      txHash: json['tx_hash'] as String,
      outputIndex: json['output_index'] as int,
      amount: json['amount'] as int,
      amountXmr: json['amount_xmr'] as String,
      key: json['key'] as String,
      keyOffset: json['key_offset'] as String,
      commitmentMask: json['commitment_mask'] as String,
      subaddressIndex: json['subaddress_index'] != null
          ? (json['subaddress_index'] as List).cast<int>()
          : null,
      paymentId: json['payment_id'] as String?,
      receivedOutputBytes: json['received_output_bytes'] as String,
      blockHeight: json['block_height'] as int,
      spent: json['spent'] as bool,
      keyImage: json['key_image'] as String,
      isCoinbase: json['is_coinbase'] as bool,
      frozen: json['frozen'] as bool,
    );
  }

  Map<String, dynamic> toJson() => {
        'tx_hash': txHash,
        'output_index': outputIndex,
        'amount': amount,
        'amount_xmr': amountXmr,
        'key': key,
        'key_offset': keyOffset,
        'commitment_mask': commitmentMask,
        'subaddress_index': subaddressIndex,
        'payment_id': paymentId,
        'received_output_bytes': receivedOutputBytes,
        'block_height': blockHeight,
        'spent': spent,
        'key_image': keyImage,
        'is_coinbase': isCoinbase,
        'frozen': frozen,
      };

  OwnedOutput copyWith({
    String? txHash,
    int? outputIndex,
    int? amount,
    String? amountXmr,
    String? key,
    String? keyOffset,
    String? commitmentMask,
    List<int>? subaddressIndex,
    String? paymentId,
    String? receivedOutputBytes,
    int? blockHeight,
    bool? spent,
    String? keyImage,
    bool? isCoinbase,
    bool? frozen,
  }) {
    return OwnedOutput(
      txHash: txHash ?? this.txHash,
      outputIndex: outputIndex ?? this.outputIndex,
      amount: amount ?? this.amount,
      amountXmr: amountXmr ?? this.amountXmr,
      key: key ?? this.key,
      keyOffset: keyOffset ?? this.keyOffset,
      commitmentMask: commitmentMask ?? this.commitmentMask,
      subaddressIndex: subaddressIndex ?? this.subaddressIndex,
      paymentId: paymentId ?? this.paymentId,
      receivedOutputBytes: receivedOutputBytes ?? this.receivedOutputBytes,
      blockHeight: blockHeight ?? this.blockHeight,
      spent: spent ?? this.spent,
      keyImage: keyImage ?? this.keyImage,
      isCoinbase: isCoinbase ?? this.isCoinbase,
      frozen: frozen ?? this.frozen,
    );
  }
}

class ChangeOutput {
  final String txHash;
  final int outputIndex;
  final int amount;
  final String amountXmr;
  final String key;
  final String keyOffset;
  final String commitmentMask;
  final List<int>? subaddressIndex;
  final String receivedOutputBytes;
  final String keyImage;

  ChangeOutput({
    required this.txHash,
    required this.outputIndex,
    required this.amount,
    required this.amountXmr,
    required this.key,
    required this.keyOffset,
    required this.commitmentMask,
    this.subaddressIndex,
    required this.receivedOutputBytes,
    required this.keyImage,
  });

  factory ChangeOutput.fromJson(Map<String, dynamic> json) {
    return ChangeOutput(
      txHash: json['tx_hash'] as String,
      outputIndex: json['output_index'] as int,
      amount: json['amount'] as int,
      amountXmr: json['amount_xmr'] as String,
      key: json['key'] as String,
      keyOffset: json['key_offset'] as String,
      commitmentMask: json['commitment_mask'] as String,
      subaddressIndex: json['subaddress_index'] != null
          ? (json['subaddress_index'] as List).cast<int>()
          : null,
      receivedOutputBytes: json['received_output_bytes'] as String,
      keyImage: json['key_image'] as String,
    );
  }

  Map<String, dynamic> toJson() => {
        'tx_hash': txHash,
        'output_index': outputIndex,
        'amount': amount,
        'amount_xmr': amountXmr,
        'key': key,
        'key_offset': keyOffset,
        'commitment_mask': commitmentMask,
        'subaddress_index': subaddressIndex,
        'received_output_bytes': receivedOutputBytes,
        'key_image': keyImage,
      };
}

class WalletConfig {
  final String seed;
  final String network;
  final int accountLookahead;
  final int subaddressLookahead;
  final List<int>? accountsToScan;
  final String passphrase;
  final int bip39AccountIndex;

  WalletConfig({
    required this.seed,
    required this.network,
    required this.accountLookahead,
    this.subaddressLookahead = 0,
    this.accountsToScan,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  factory WalletConfig.fromJson(Map<String, dynamic> json) {
    return WalletConfig(
      seed: json['seed'] as String,
      network: json['network'] as String,
      accountLookahead: json['account_lookahead'] as int,
      subaddressLookahead: (json['subaddress_lookahead'] as int?) ?? 0,
      accountsToScan: json['accounts_to_scan'] != null
          ? (json['accounts_to_scan'] as List).cast<int>()
          : null,
      passphrase: (json['passphrase'] as String?) ?? '',
      bip39AccountIndex: (json['bip39_account_index'] as int?) ?? 0,
    );
  }

  Map<String, dynamic> toJson() => {
        'seed': seed,
        'network': network,
        'account_lookahead': accountLookahead,
        'subaddress_lookahead': subaddressLookahead,
        'accounts_to_scan': accountsToScan,
        'passphrase': passphrase,
        'bip39_account_index': bip39AccountIndex,
      };
}

class WalletScanResult {
  final String address;
  final List<OwnedOutput> outputs;

  WalletScanResult({required this.address, required this.outputs});

  factory WalletScanResult.fromJson(Map<String, dynamic> json) {
    return WalletScanResult(
      address: json['address'] as String,
      outputs: (json['outputs'] as List)
          .map((e) => OwnedOutput.fromJson(e as Map<String, dynamic>))
          .toList(),
    );
  }

  Map<String, dynamic> toJson() => {
        'address': address,
        'outputs': outputs.map((e) => e.toJson()).toList(),
      };
}

class DoubleSpendConflict {
  final String keyImage;
  final int previousSpentHeight;
  final int newHeight;

  DoubleSpendConflict({
    required this.keyImage,
    required this.previousSpentHeight,
    required this.newHeight,
  });

  factory DoubleSpendConflict.fromJson(Map<String, dynamic> json) {
    return DoubleSpendConflict(
      keyImage: json['key_image'] as String,
      previousSpentHeight: json['previous_spent_height'] as int,
      newHeight: json['new_height'] as int,
    );
  }

  Map<String, dynamic> toJson() => {
        'key_image': keyImage,
        'previous_spent_height': previousSpentHeight,
        'new_height': newHeight,
      };
}

// ===========================================================================
// DartSignal types (Dart → Rust requests)
// ===========================================================================

class MoneroTestRequest {
  MoneroTestRequest();

  Map<String, dynamic> toJson() => {};

  void sendSignalToRust() {
    ffi.sendDartSignalJson('monero_test_request', jsonEncode(toJson()));
  }
}

class CreateWalletRequest {
  final String password;
  final String network;

  CreateWalletRequest({required this.password, required this.network});

  Map<String, dynamic> toJson() => {
        'password': password,
        'network': network,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson('create_wallet_request', jsonEncode(toJson()));
  }
}

class StartSyncRequest {
  StartSyncRequest();

  Map<String, dynamic> toJson() => {};

  void sendSignalToRust() {
    ffi.sendDartSignalJson('start_sync_request', jsonEncode(toJson()));
  }
}

class GetBalanceRequest {
  GetBalanceRequest();

  Map<String, dynamic> toJson() => {};

  void sendSignalToRust() {
    ffi.sendDartSignalJson('get_balance_request', jsonEncode(toJson()));
  }
}

class CreateTransactionRequest {
  final String nodeUrl;
  final String seed;
  final String network;
  final List<Recipient> recipients;
  final List<String>? selectedOutputs;
  final String passphrase;
  final int bip39AccountIndex;
  final bool subtractFee;

  CreateTransactionRequest({
    required this.nodeUrl,
    required this.seed,
    required this.network,
    required this.recipients,
    this.selectedOutputs,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
    this.subtractFee = false,
  });

  Map<String, dynamic> toJson() => {
        'node_url': nodeUrl,
        'seed': seed,
        'network': network,
        'recipients': recipients.map((e) => e.toJson()).toList(),
        'selected_outputs': selectedOutputs,
        'passphrase': passphrase,
        'bip39_account_index': bip39AccountIndex,
        'subtract_fee': subtractFee,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'create_transaction_request', jsonEncode(toJson()));
  }
}

class SweepAllRequest {
  final String nodeUrl;
  final String seed;
  final String network;
  final String destinationAddress;
  final List<String>? selectedOutputs;
  final String passphrase;
  final int bip39AccountIndex;

  SweepAllRequest({
    required this.nodeUrl,
    required this.seed,
    required this.network,
    required this.destinationAddress,
    this.selectedOutputs,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  Map<String, dynamic> toJson() => {
        'node_url': nodeUrl,
        'seed': seed,
        'network': network,
        'destination_address': destinationAddress,
        'selected_outputs': selectedOutputs,
        'passphrase': passphrase,
        'bip39_account_index': bip39AccountIndex,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson('sweep_all_request', jsonEncode(toJson()));
  }
}

class GenerateSeedRequest {
  final String seedType;

  GenerateSeedRequest({required this.seedType});

  Map<String, dynamic> toJson() => {
        'seed_type': seedType,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson('generate_seed_request', jsonEncode(toJson()));
  }
}

class GetSeedBirthdayRequest {
  final String seed;
  final String passphrase;
  final int bip39AccountIndex;

  GetSeedBirthdayRequest({
    required this.seed,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  Map<String, dynamic> toJson() => {
        'seed': seed,
        'passphrase': passphrase,
        'bip39_account_index': bip39AccountIndex,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'get_seed_birthday_request', jsonEncode(toJson()));
  }
}

class GetBlockHeightFromTimestampRequest {
  final int timestamp;
  final String nodeUrl;

  GetBlockHeightFromTimestampRequest({
    required this.timestamp,
    required this.nodeUrl,
  });

  Map<String, dynamic> toJson() => {
        'timestamp': timestamp,
        'node_url': nodeUrl,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'get_block_height_from_timestamp_request', jsonEncode(toJson()));
  }
}

class DeriveAddressRequest {
  final String seed;
  final String network;
  final String passphrase;
  final int bip39AccountIndex;

  DeriveAddressRequest({
    required this.seed,
    required this.network,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  Map<String, dynamic> toJson() => {
        'seed': seed,
        'network': network,
        'passphrase': passphrase,
        'bip39_account_index': bip39AccountIndex,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson('derive_address_request', jsonEncode(toJson()));
  }
}

class DeriveSubaddressRequest {
  final String seed;
  final String network;
  final int account;
  final int addressIndex;
  final String passphrase;
  final int bip39AccountIndex;

  DeriveSubaddressRequest({
    required this.seed,
    required this.network,
    required this.account,
    required this.addressIndex,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  Map<String, dynamic> toJson() => {
        'seed': seed,
        'network': network,
        'account': account,
        'address_index': addressIndex,
        'passphrase': passphrase,
        'bip39_account_index': bip39AccountIndex,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'derive_subaddress_request', jsonEncode(toJson()));
  }
}

class DeriveKeysRequest {
  final String seed;
  final String network;
  final String passphrase;
  final int bip39AccountIndex;

  DeriveKeysRequest({
    required this.seed,
    required this.network,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  Map<String, dynamic> toJson() => {
        'seed': seed,
        'network': network,
        'passphrase': passphrase,
        'bip39_account_index': bip39AccountIndex,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson('derive_keys_request', jsonEncode(toJson()));
  }
}

class ScanBlockRequest {
  final String nodeUrl;
  final int blockHeight;
  final String seed;
  final String network;
  final String passphrase;
  final int bip39AccountIndex;

  ScanBlockRequest({
    required this.nodeUrl,
    required this.blockHeight,
    required this.seed,
    required this.network,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  Map<String, dynamic> toJson() => {
        'node_url': nodeUrl,
        'block_height': blockHeight,
        'seed': seed,
        'network': network,
        'passphrase': passphrase,
        'bip39_account_index': bip39AccountIndex,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson('scan_block_request', jsonEncode(toJson()));
  }
}

class BroadcastTransactionRequest {
  final String nodeUrl;
  final String txBlob;
  final List<String> spentOutputHashes;
  final String txId;
  final List<String> spentKeyImages;

  BroadcastTransactionRequest({
    required this.nodeUrl,
    required this.txBlob,
    required this.spentOutputHashes,
    this.txId = '',
    this.spentKeyImages = const [],
  });

  Map<String, dynamic> toJson() => {
        'node_url': nodeUrl,
        'tx_blob': txBlob,
        'spent_output_hashes': spentOutputHashes,
        'tx_id': txId,
        'spent_key_images': spentKeyImages,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'broadcast_transaction_request', jsonEncode(toJson()));
  }
}

class QueryDaemonHeightRequest {
  final String nodeUrl;

  QueryDaemonHeightRequest({required this.nodeUrl});

  Map<String, dynamic> toJson() => {
        'node_url': nodeUrl,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'query_daemon_height_request', jsonEncode(toJson()));
  }
}

class StartContinuousScanRequest {
  final String nodeUrl;
  final int startHeight;
  final String seed;
  final String network;
  final int accountLookahead;
  final int subaddressLookahead;
  final List<int>? accountsToScan;
  final String passphrase;
  final int bip39AccountIndex;

  StartContinuousScanRequest({
    required this.nodeUrl,
    required this.startHeight,
    required this.seed,
    required this.network,
    required this.accountLookahead,
    this.subaddressLookahead = 0,
    this.accountsToScan,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  Map<String, dynamic> toJson() => {
        'node_url': nodeUrl,
        'start_height': startHeight,
        'seed': seed,
        'network': network,
        'account_lookahead': accountLookahead,
        'subaddress_lookahead': subaddressLookahead,
        'accounts_to_scan': accountsToScan,
        'passphrase': passphrase,
        'bip39_account_index': bip39AccountIndex,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'start_continuous_scan_request', jsonEncode(toJson()));
  }
}

class StopScanRequest {
  StopScanRequest();

  Map<String, dynamic> toJson() => {};

  void sendSignalToRust() {
    ffi.sendDartSignalJson('stop_scan_request', jsonEncode(toJson()));
  }
}

class MempoolScanRequest {
  final String nodeUrl;
  final String seed;
  final String network;
  final int accountLookahead;
  final int subaddressLookahead;
  final List<int>? accountsToScan;
  final String passphrase;
  final int bip39AccountIndex;

  MempoolScanRequest({
    required this.nodeUrl,
    required this.seed,
    required this.network,
    required this.accountLookahead,
    this.subaddressLookahead = 0,
    this.accountsToScan,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  Map<String, dynamic> toJson() => {
        'node_url': nodeUrl,
        'seed': seed,
        'network': network,
        'account_lookahead': accountLookahead,
        'subaddress_lookahead': subaddressLookahead,
        'accounts_to_scan': accountsToScan,
        'passphrase': passphrase,
        'bip39_account_index': bip39AccountIndex,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson('mempool_scan_request', jsonEncode(toJson()));
  }
}

class GenerateOutProofRequest {
  final String txId;
  final String txKey;
  final String recipientAddress;
  final String message;
  final String network;

  GenerateOutProofRequest({
    required this.txId,
    required this.txKey,
    required this.recipientAddress,
    required this.message,
    required this.network,
  });

  Map<String, dynamic> toJson() => {
        'tx_id': txId,
        'tx_key': txKey,
        'recipient_address': recipientAddress,
        'message': message,
        'network': network,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'generate_out_proof_request', jsonEncode(toJson()));
  }
}

class SaveWalletDataRequest {
  final String password;
  final String walletDataJson;

  SaveWalletDataRequest({
    required this.password,
    required this.walletDataJson,
  });

  Map<String, dynamic> toJson() => {
        'password': password,
        'wallet_data_json': walletDataJson,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'save_wallet_data_request', jsonEncode(toJson()));
  }
}

class LoadWalletDataRequest {
  final String password;
  final String encryptedData;

  LoadWalletDataRequest({
    required this.password,
    required this.encryptedData,
  });

  Map<String, dynamic> toJson() => {
        'password': password,
        'encrypted_data': encryptedData,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'load_wallet_data_request', jsonEncode(toJson()));
  }
}

class DeriveEncryptionKeyRequest {
  final String password;

  DeriveEncryptionKeyRequest({required this.password});

  Map<String, dynamic> toJson() => {
        'password': password,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'derive_encryption_key_request', jsonEncode(toJson()));
  }
}

class SaveWithDerivedKeyRequest {
  final String keyHex;
  final String saltHex;
  final String walletDataJson;

  SaveWithDerivedKeyRequest({
    required this.keyHex,
    required this.saltHex,
    required this.walletDataJson,
  });

  Map<String, dynamic> toJson() => {
        'key_hex': keyHex,
        'salt_hex': saltHex,
        'wallet_data_json': walletDataJson,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'save_with_derived_key_request', jsonEncode(toJson()));
  }
}

class ScanBlockMultiWalletRequest {
  final String nodeUrl;
  final int blockHeight;
  final List<WalletConfig> wallets;

  ScanBlockMultiWalletRequest({
    required this.nodeUrl,
    required this.blockHeight,
    required this.wallets,
  });

  Map<String, dynamic> toJson() => {
        'node_url': nodeUrl,
        'block_height': blockHeight,
        'wallets': wallets.map((e) => e.toJson()).toList(),
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'scan_block_multi_wallet_request', jsonEncode(toJson()));
  }
}

class StartMultiWalletScanRequest {
  final String nodeUrl;
  final int startHeight;
  final List<WalletConfig> wallets;

  StartMultiWalletScanRequest({
    required this.nodeUrl,
    required this.startHeight,
    required this.wallets,
  });

  Map<String, dynamic> toJson() => {
        'node_url': nodeUrl,
        'start_height': startHeight,
        'wallets': wallets.map((e) => e.toJson()).toList(),
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'start_multi_wallet_scan_request', jsonEncode(toJson()));
  }
}

class RestoreWalletDataRequest {
  final String seed;
  final String network;
  final List<OwnedOutput> outputs;
  final int daemonHeight;
  final int currentHeight;
  final String? blockHashesJson;
  final String? pendingStateJson;
  final String passphrase;
  final int bip39AccountIndex;

  RestoreWalletDataRequest({
    required this.seed,
    required this.network,
    required this.outputs,
    required this.daemonHeight,
    required this.currentHeight,
    this.blockHashesJson,
    this.pendingStateJson,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  Map<String, dynamic> toJson() => {
        'seed': seed,
        'network': network,
        'outputs': outputs.map((e) => e.toJson()).toList(),
        'daemon_height': daemonHeight,
        'current_height': currentHeight,
        'block_hashes_json': blockHashesJson,
        'pending_state_json': pendingStateJson,
        'passphrase': passphrase,
        'bip39_account_index': bip39AccountIndex,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'restore_wallet_data_request', jsonEncode(toJson()));
  }
}

class GetBlockHashesRequest {
  GetBlockHashesRequest();

  Map<String, dynamic> toJson() => {};

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'get_block_hashes_request', jsonEncode(toJson()));
  }
}

class GetPendingStateRequest {
  GetPendingStateRequest();

  Map<String, dynamic> toJson() => {};

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'get_pending_state_request', jsonEncode(toJson()));
  }
}

class ConvertBip39ToLegacyRequest {
  final String bip39Mnemonic;
  final int accountIndex;
  final String passphrase;

  ConvertBip39ToLegacyRequest({
    required this.bip39Mnemonic,
    required this.accountIndex,
    this.passphrase = '',
  });

  Map<String, dynamic> toJson() => {
        'bip39_mnemonic': bip39Mnemonic,
        'account_index': accountIndex,
        'passphrase': passphrase,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'convert_bip39_to_legacy_request', jsonEncode(toJson()));
  }
}

class FreezeOutputRequest {
  final String keyImage;

  FreezeOutputRequest({required this.keyImage});

  Map<String, dynamic> toJson() => {
        'key_image': keyImage,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson('freeze_output_request', jsonEncode(toJson()));
  }
}

class ThawOutputRequest {
  final String keyImage;

  ThawOutputRequest({required this.keyImage});

  Map<String, dynamic> toJson() => {
        'key_image': keyImage,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson('thaw_output_request', jsonEncode(toJson()));
  }
}

class CreateUnsignedTransactionRequest {
  final String nodeUrl;
  final String viewKeyHex;
  final String pubSpendKeyHex;
  final String network;
  final List<Recipient> recipients;
  final List<String>? selectedOutputs;

  CreateUnsignedTransactionRequest({
    required this.nodeUrl,
    required this.viewKeyHex,
    required this.pubSpendKeyHex,
    required this.network,
    required this.recipients,
    this.selectedOutputs,
  });

  Map<String, dynamic> toJson() => {
        'node_url': nodeUrl,
        'view_key_hex': viewKeyHex,
        'pub_spend_key_hex': pubSpendKeyHex,
        'network': network,
        'recipients': recipients.map((e) => e.toJson()).toList(),
        'selected_outputs': selectedOutputs,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'create_unsigned_transaction_request', jsonEncode(toJson()));
  }
}

class SignUnsignedTransactionRequest {
  final String seed;
  final String unsignedTxHex;
  final String network;
  final String passphrase;
  final int bip39AccountIndex;

  SignUnsignedTransactionRequest({
    required this.seed,
    required this.unsignedTxHex,
    required this.network,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  Map<String, dynamic> toJson() => {
        'seed': seed,
        'unsigned_tx_hex': unsignedTxHex,
        'network': network,
        'passphrase': passphrase,
        'bip39_account_index': bip39AccountIndex,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'sign_unsigned_transaction_request', jsonEncode(toJson()));
  }
}

class ExportKeyImagesRequest {
  final String seed;
  final String network;
  final String passphrase;
  final int bip39AccountIndex;

  ExportKeyImagesRequest({
    required this.seed,
    required this.network,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  Map<String, dynamic> toJson() => {
        'seed': seed,
        'network': network,
        'passphrase': passphrase,
        'bip39_account_index': bip39AccountIndex,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'export_key_images_request', jsonEncode(toJson()));
  }
}

class ImportKeyImagesRequest {
  final String dataHex;
  final String nodeUrl;

  ImportKeyImagesRequest({required this.dataHex, required this.nodeUrl});

  Map<String, dynamic> toJson() => {
        'data_hex': dataHex,
        'node_url': nodeUrl,
      };

  void sendSignalToRust() {
    ffi.sendDartSignalJson(
        'import_key_images_request', jsonEncode(toJson()));
  }
}

// ===========================================================================
// RustSignal types (Rust → Dart responses)
// ===========================================================================

class MoneroTestResponse {
  final String result;

  MoneroTestResponse({required this.result});

  factory MoneroTestResponse.fromJson(Map<String, dynamic> json) {
    return MoneroTestResponse(result: json['result'] as String);
  }

  static final _controller =
      StreamController<MoneroTestResponse>.broadcast();
  static Stream<MoneroTestResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(
        MoneroTestResponse.fromJson(jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class WalletCreatedResponse {
  final String address;

  WalletCreatedResponse({required this.address});

  factory WalletCreatedResponse.fromJson(Map<String, dynamic> json) {
    return WalletCreatedResponse(address: json['address'] as String);
  }

  static final _controller =
      StreamController<WalletCreatedResponse>.broadcast();
  static Stream<WalletCreatedResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(WalletCreatedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class SyncProgressResponse {
  final int currentHeight;
  final int daemonHeight;
  final bool isSynced;
  final bool isScanning;

  SyncProgressResponse({
    required this.currentHeight,
    required this.daemonHeight,
    required this.isSynced,
    required this.isScanning,
  });

  factory SyncProgressResponse.fromJson(Map<String, dynamic> json) {
    return SyncProgressResponse(
      currentHeight: json['current_height'] as int,
      daemonHeight: json['daemon_height'] as int,
      isSynced: json['is_synced'] as bool,
      isScanning: json['is_scanning'] as bool,
    );
  }

  static final _controller =
      StreamController<SyncProgressResponse>.broadcast();
  static Stream<SyncProgressResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(SyncProgressResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class BalanceResponse {
  final int confirmed;
  final int unconfirmed;
  final int pendingSpend;

  BalanceResponse({
    required this.confirmed,
    required this.unconfirmed,
    required this.pendingSpend,
  });

  factory BalanceResponse.fromJson(Map<String, dynamic> json) {
    return BalanceResponse(
      confirmed: json['confirmed'] as int,
      unconfirmed: json['unconfirmed'] as int,
      pendingSpend: json['pending_spend'] as int,
    );
  }

  static final _controller =
      StreamController<BalanceResponse>.broadcast();
  static Stream<BalanceResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(
        BalanceResponse.fromJson(jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class TransactionCreatedResponse {
  final bool success;
  final String? error;
  final String txId;
  final int fee;
  final String? txBlob;
  final String? txKey;
  final List<String> txKeyAdditional;
  final List<String> spentOutputHashes;
  final List<ChangeOutput> changeOutputs;

  TransactionCreatedResponse({
    required this.success,
    this.error,
    required this.txId,
    required this.fee,
    this.txBlob,
    this.txKey,
    required this.txKeyAdditional,
    required this.spentOutputHashes,
    required this.changeOutputs,
  });

  factory TransactionCreatedResponse.fromJson(Map<String, dynamic> json) {
    return TransactionCreatedResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      txId: json['tx_id'] as String,
      fee: json['fee'] as int,
      txBlob: json['tx_blob'] as String?,
      txKey: json['tx_key'] as String?,
      txKeyAdditional:
          (json['tx_key_additional'] as List).cast<String>(),
      spentOutputHashes:
          (json['spent_output_hashes'] as List).cast<String>(),
      changeOutputs: (json['change_outputs'] as List)
          .map((e) => ChangeOutput.fromJson(e as Map<String, dynamic>))
          .toList(),
    );
  }

  static final _controller =
      StreamController<TransactionCreatedResponse>.broadcast();
  static Stream<TransactionCreatedResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(TransactionCreatedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class SeedGeneratedResponse {
  final String seed;
  final bool success;
  final String? error;
  final int? restoreHeight;

  SeedGeneratedResponse({
    required this.seed,
    required this.success,
    this.error,
    this.restoreHeight,
  });

  factory SeedGeneratedResponse.fromJson(Map<String, dynamic> json) {
    return SeedGeneratedResponse(
      seed: json['seed'] as String,
      success: json['success'] as bool,
      error: json['error'] as String?,
      restoreHeight: json['restore_height'] as int?,
    );
  }

  static final _controller =
      StreamController<SeedGeneratedResponse>.broadcast();
  static Stream<SeedGeneratedResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(SeedGeneratedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class SeedBirthdayResponse {
  final int? birthday;
  final bool success;
  final String? error;

  SeedBirthdayResponse({
    this.birthday,
    required this.success,
    this.error,
  });

  factory SeedBirthdayResponse.fromJson(Map<String, dynamic> json) {
    return SeedBirthdayResponse(
      birthday: json['birthday'] as int?,
      success: json['success'] as bool,
      error: json['error'] as String?,
    );
  }

  static final _controller =
      StreamController<SeedBirthdayResponse>.broadcast();
  static Stream<SeedBirthdayResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(SeedBirthdayResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class BlockHeightFromTimestampResponse {
  final int blockHeight;
  final bool success;
  final String? error;

  BlockHeightFromTimestampResponse({
    required this.blockHeight,
    required this.success,
    this.error,
  });

  factory BlockHeightFromTimestampResponse.fromJson(
      Map<String, dynamic> json) {
    return BlockHeightFromTimestampResponse(
      blockHeight: json['block_height'] as int,
      success: json['success'] as bool,
      error: json['error'] as String?,
    );
  }

  static final _controller =
      StreamController<BlockHeightFromTimestampResponse>.broadcast();
  static Stream<BlockHeightFromTimestampResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(BlockHeightFromTimestampResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class AddressDerivedResponse {
  final String address;
  final bool success;
  final String? error;

  AddressDerivedResponse({
    required this.address,
    required this.success,
    this.error,
  });

  factory AddressDerivedResponse.fromJson(Map<String, dynamic> json) {
    return AddressDerivedResponse(
      address: json['address'] as String,
      success: json['success'] as bool,
      error: json['error'] as String?,
    );
  }

  static final _controller =
      StreamController<AddressDerivedResponse>.broadcast();
  static Stream<AddressDerivedResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(AddressDerivedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class SubaddressDerivedResponse {
  final String address;
  final bool success;
  final String? error;

  SubaddressDerivedResponse({
    required this.address,
    required this.success,
    this.error,
  });

  factory SubaddressDerivedResponse.fromJson(Map<String, dynamic> json) {
    return SubaddressDerivedResponse(
      address: json['address'] as String,
      success: json['success'] as bool,
      error: json['error'] as String?,
    );
  }

  static final _controller =
      StreamController<SubaddressDerivedResponse>.broadcast();
  static Stream<SubaddressDerivedResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(SubaddressDerivedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class KeysDerivedResponse {
  final String address;
  final String secretSpendKey;
  final String secretViewKey;
  final String publicSpendKey;
  final String publicViewKey;
  final bool success;
  final String? error;

  KeysDerivedResponse({
    required this.address,
    required this.secretSpendKey,
    required this.secretViewKey,
    required this.publicSpendKey,
    required this.publicViewKey,
    required this.success,
    this.error,
  });

  factory KeysDerivedResponse.fromJson(Map<String, dynamic> json) {
    return KeysDerivedResponse(
      address: json['address'] as String,
      secretSpendKey: json['secret_spend_key'] as String,
      secretViewKey: json['secret_view_key'] as String,
      publicSpendKey: json['public_spend_key'] as String,
      publicViewKey: json['public_view_key'] as String,
      success: json['success'] as bool,
      error: json['error'] as String?,
    );
  }

  static final _controller =
      StreamController<KeysDerivedResponse>.broadcast();
  static Stream<KeysDerivedResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(KeysDerivedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class BlockScanResponse {
  final bool success;
  final String? error;
  final int blockHeight;
  final String blockHash;
  final int blockTimestamp;
  final int txCount;
  final List<OwnedOutput> outputs;
  final int daemonHeight;
  final List<String> spentKeyImages;
  final List<String> spentKeyImageTxHashes;

  BlockScanResponse({
    required this.success,
    this.error,
    required this.blockHeight,
    required this.blockHash,
    required this.blockTimestamp,
    required this.txCount,
    required this.outputs,
    required this.daemonHeight,
    required this.spentKeyImages,
    required this.spentKeyImageTxHashes,
  });

  factory BlockScanResponse.fromJson(Map<String, dynamic> json) {
    return BlockScanResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      blockHeight: json['block_height'] as int,
      blockHash: json['block_hash'] as String,
      blockTimestamp: json['block_timestamp'] as int,
      txCount: json['tx_count'] as int,
      outputs: (json['outputs'] as List)
          .map((e) => OwnedOutput.fromJson(e as Map<String, dynamic>))
          .toList(),
      daemonHeight: json['daemon_height'] as int,
      spentKeyImages:
          (json['spent_key_images'] as List).cast<String>(),
      spentKeyImageTxHashes:
          (json['spent_key_image_tx_hashes'] as List).cast<String>(),
    );
  }

  Map<String, dynamic> toJson() => {
        'success': success,
        'error': error,
        'block_height': blockHeight,
        'block_hash': blockHash,
        'block_timestamp': blockTimestamp,
        'tx_count': txCount,
        'outputs': outputs.map((e) => e.toJson()).toList(),
        'daemon_height': daemonHeight,
        'spent_key_images': spentKeyImages,
        'spent_key_image_tx_hashes': spentKeyImageTxHashes,
      };

  static final _controller =
      StreamController<BlockScanResponse>.broadcast();
  static Stream<BlockScanResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(BlockScanResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class TransactionBroadcastResponse {
  final bool success;
  final String? error;
  final String? txId;
  final bool isRetryable;
  final bool isDoubleSpend;

  TransactionBroadcastResponse({
    required this.success,
    this.error,
    this.txId,
    this.isRetryable = false,
    this.isDoubleSpend = false,
  });

  factory TransactionBroadcastResponse.fromJson(
      Map<String, dynamic> json) {
    return TransactionBroadcastResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      txId: json['tx_id'] as String?,
      isRetryable: (json['is_retryable'] as bool?) ?? false,
      isDoubleSpend: (json['is_double_spend'] as bool?) ?? false,
    );
  }

  static final _controller =
      StreamController<TransactionBroadcastResponse>.broadcast();
  static Stream<TransactionBroadcastResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(TransactionBroadcastResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class DaemonHeightResponse {
  final bool success;
  final String? error;
  final int daemonHeight;

  DaemonHeightResponse({
    required this.success,
    this.error,
    required this.daemonHeight,
  });

  factory DaemonHeightResponse.fromJson(Map<String, dynamic> json) {
    return DaemonHeightResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      daemonHeight: json['daemon_height'] as int,
    );
  }

  static final _controller =
      StreamController<DaemonHeightResponse>.broadcast();
  static Stream<DaemonHeightResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(DaemonHeightResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class SpentStatusUpdatedResponse {
  final List<String> spentKeyImages;

  SpentStatusUpdatedResponse({required this.spentKeyImages});

  factory SpentStatusUpdatedResponse.fromJson(Map<String, dynamic> json) {
    return SpentStatusUpdatedResponse(
      spentKeyImages:
          (json['spent_key_images'] as List).cast<String>(),
    );
  }

  static final _controller =
      StreamController<SpentStatusUpdatedResponse>.broadcast();
  static Stream<SpentStatusUpdatedResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(SpentStatusUpdatedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class MempoolScanResponse {
  final bool success;
  final String? error;
  final int txCount;
  final List<OwnedOutput> outputs;
  final List<String> spentKeyImages;
  final List<String> spentKeyImageTxHashes;

  MempoolScanResponse({
    required this.success,
    this.error,
    required this.txCount,
    required this.outputs,
    required this.spentKeyImages,
    required this.spentKeyImageTxHashes,
  });

  factory MempoolScanResponse.fromJson(Map<String, dynamic> json) {
    return MempoolScanResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      txCount: json['tx_count'] as int,
      outputs: (json['outputs'] as List)
          .map((e) => OwnedOutput.fromJson(e as Map<String, dynamic>))
          .toList(),
      spentKeyImages:
          (json['spent_key_images'] as List).cast<String>(),
      spentKeyImageTxHashes:
          (json['spent_key_image_tx_hashes'] as List).cast<String>(),
    );
  }

  static final _controller =
      StreamController<MempoolScanResponse>.broadcast();
  static Stream<MempoolScanResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(MempoolScanResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class OutProofGeneratedResponse {
  final bool success;
  final String? error;
  final String? signature;
  final String? formatted;

  OutProofGeneratedResponse({
    required this.success,
    this.error,
    this.signature,
    this.formatted,
  });

  factory OutProofGeneratedResponse.fromJson(Map<String, dynamic> json) {
    return OutProofGeneratedResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      signature: json['signature'] as String?,
      formatted: json['formatted'] as String?,
    );
  }

  static final _controller =
      StreamController<OutProofGeneratedResponse>.broadcast();
  static Stream<OutProofGeneratedResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(OutProofGeneratedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class WalletDataSavedResponse {
  final bool success;
  final String? error;
  final String? encryptedData;

  WalletDataSavedResponse({
    required this.success,
    this.error,
    this.encryptedData,
  });

  factory WalletDataSavedResponse.fromJson(Map<String, dynamic> json) {
    return WalletDataSavedResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      encryptedData: json['encrypted_data'] as String?,
    );
  }

  static final _controller =
      StreamController<WalletDataSavedResponse>.broadcast();
  static Stream<WalletDataSavedResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(WalletDataSavedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class WalletDataLoadedResponse {
  final bool success;
  final String? error;
  final String? walletDataJson;

  WalletDataLoadedResponse({
    required this.success,
    this.error,
    this.walletDataJson,
  });

  factory WalletDataLoadedResponse.fromJson(Map<String, dynamic> json) {
    return WalletDataLoadedResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      walletDataJson: json['wallet_data_json'] as String?,
    );
  }

  static final _controller =
      StreamController<WalletDataLoadedResponse>.broadcast();
  static Stream<WalletDataLoadedResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(WalletDataLoadedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class EncryptionKeyDerivedResponse {
  final bool success;
  final String? error;
  final String? keyHex;
  final String? saltHex;

  EncryptionKeyDerivedResponse({
    required this.success,
    this.error,
    this.keyHex,
    this.saltHex,
  });

  factory EncryptionKeyDerivedResponse.fromJson(
      Map<String, dynamic> json) {
    return EncryptionKeyDerivedResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      keyHex: json['key_hex'] as String?,
      saltHex: json['salt_hex'] as String?,
    );
  }

  static final _controller =
      StreamController<EncryptionKeyDerivedResponse>.broadcast();
  static Stream<EncryptionKeyDerivedResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(EncryptionKeyDerivedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class MultiWalletScanResponse {
  final bool success;
  final String? error;
  final int blockHeight;
  final String blockHash;
  final int blockTimestamp;
  final int txCount;
  final int daemonHeight;
  final List<String> spentKeyImages;
  final List<String> spentKeyImageTxHashes;
  final List<WalletScanResult> walletResults;

  MultiWalletScanResponse({
    required this.success,
    this.error,
    required this.blockHeight,
    required this.blockHash,
    required this.blockTimestamp,
    required this.txCount,
    required this.daemonHeight,
    required this.spentKeyImages,
    required this.spentKeyImageTxHashes,
    required this.walletResults,
  });

  factory MultiWalletScanResponse.fromJson(Map<String, dynamic> json) {
    return MultiWalletScanResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      blockHeight: json['block_height'] as int,
      blockHash: json['block_hash'] as String,
      blockTimestamp: json['block_timestamp'] as int,
      txCount: json['tx_count'] as int,
      daemonHeight: json['daemon_height'] as int,
      spentKeyImages:
          (json['spent_key_images'] as List).cast<String>(),
      spentKeyImageTxHashes:
          (json['spent_key_image_tx_hashes'] as List).cast<String>(),
      walletResults: (json['wallet_results'] as List)
          .map((e) =>
              WalletScanResult.fromJson(e as Map<String, dynamic>))
          .toList(),
    );
  }

  static final _controller =
      StreamController<MultiWalletScanResponse>.broadcast();
  static Stream<MultiWalletScanResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(MultiWalletScanResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class BlockHashesResponse {
  final bool success;
  final String? error;
  final String? blockHashesJson;

  BlockHashesResponse({
    required this.success,
    this.error,
    this.blockHashesJson,
  });

  factory BlockHashesResponse.fromJson(Map<String, dynamic> json) {
    return BlockHashesResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      blockHashesJson: json['block_hashes_json'] as String?,
    );
  }

  static final _controller =
      StreamController<BlockHashesResponse>.broadcast();
  static Stream<BlockHashesResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(BlockHashesResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class PendingStateResponse {
  final bool success;
  final String? error;
  final String? pendingStateJson;

  PendingStateResponse({
    required this.success,
    this.error,
    this.pendingStateJson,
  });

  factory PendingStateResponse.fromJson(Map<String, dynamic> json) {
    return PendingStateResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      pendingStateJson: json['pending_state_json'] as String?,
    );
  }

  static final _controller =
      StreamController<PendingStateResponse>.broadcast();
  static Stream<PendingStateResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(PendingStateResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class ReorgDetectedResponse {
  final int splitHeight;
  final int blocksDetached;
  final int outputsRemoved;
  final int outputsUnspent;
  final List<String> removedKeyImages;
  final List<String> unspentKeyImages;

  ReorgDetectedResponse({
    required this.splitHeight,
    required this.blocksDetached,
    required this.outputsRemoved,
    required this.outputsUnspent,
    required this.removedKeyImages,
    required this.unspentKeyImages,
  });

  factory ReorgDetectedResponse.fromJson(Map<String, dynamic> json) {
    return ReorgDetectedResponse(
      splitHeight: json['split_height'] as int,
      blocksDetached: json['blocks_detached'] as int,
      outputsRemoved: json['outputs_removed'] as int,
      outputsUnspent: json['outputs_unspent'] as int,
      removedKeyImages:
          (json['removed_key_images'] as List).cast<String>(),
      unspentKeyImages:
          (json['unspent_key_images'] as List).cast<String>(),
    );
  }

  static final _controller =
      StreamController<ReorgDetectedResponse>.broadcast();
  static Stream<ReorgDetectedResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(ReorgDetectedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class DoubleSpendDetectedResponse {
  final List<DoubleSpendConflict> conflicts;

  DoubleSpendDetectedResponse({required this.conflicts});

  factory DoubleSpendDetectedResponse.fromJson(
      Map<String, dynamic> json) {
    return DoubleSpendDetectedResponse(
      conflicts: (json['conflicts'] as List)
          .map((e) =>
              DoubleSpendConflict.fromJson(e as Map<String, dynamic>))
          .toList(),
    );
  }

  static final _controller =
      StreamController<DoubleSpendDetectedResponse>.broadcast();
  static Stream<DoubleSpendDetectedResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(DoubleSpendDetectedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class Bip39LegacySeedResponse {
  final String legacySeed;
  final bool success;
  final String? error;

  Bip39LegacySeedResponse({
    required this.legacySeed,
    required this.success,
    this.error,
  });

  factory Bip39LegacySeedResponse.fromJson(Map<String, dynamic> json) {
    return Bip39LegacySeedResponse(
      legacySeed: json['legacy_seed'] as String,
      success: json['success'] as bool,
      error: json['error'] as String?,
    );
  }

  static final _controller =
      StreamController<Bip39LegacySeedResponse>.broadcast();
  static Stream<Bip39LegacySeedResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(Bip39LegacySeedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class FreezeThawResponse {
  final bool success;
  final String keyImage;
  final bool frozen;

  FreezeThawResponse({
    required this.success,
    required this.keyImage,
    required this.frozen,
  });

  factory FreezeThawResponse.fromJson(Map<String, dynamic> json) {
    return FreezeThawResponse(
      success: json['success'] as bool,
      keyImage: json['key_image'] as String,
      frozen: json['frozen'] as bool,
    );
  }

  static final _controller =
      StreamController<FreezeThawResponse>.broadcast();
  static Stream<FreezeThawResponse> get stream => _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(FreezeThawResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class TransactionStatusUpdate {
  final String txId;
  final String status;
  final int? confirmedHeight;

  TransactionStatusUpdate({
    required this.txId,
    required this.status,
    this.confirmedHeight,
  });

  factory TransactionStatusUpdate.fromJson(Map<String, dynamic> json) {
    return TransactionStatusUpdate(
      txId: json['tx_id'] as String,
      status: json['status'] as String,
      confirmedHeight: json['confirmed_height'] as int?,
    );
  }

  static final _controller =
      StreamController<TransactionStatusUpdate>.broadcast();
  static Stream<TransactionStatusUpdate> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(TransactionStatusUpdate.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class UnsignedTransactionCreatedResponse {
  final bool success;
  final String? error;
  final String? unsignedTxHex;
  final int fee;
  final List<Recipient> recipients;

  UnsignedTransactionCreatedResponse({
    required this.success,
    this.error,
    this.unsignedTxHex,
    required this.fee,
    required this.recipients,
  });

  factory UnsignedTransactionCreatedResponse.fromJson(
      Map<String, dynamic> json) {
    return UnsignedTransactionCreatedResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      unsignedTxHex: json['unsigned_tx_hex'] as String?,
      fee: json['fee'] as int,
      recipients: (json['recipients'] as List)
          .map((e) => Recipient.fromJson(e as Map<String, dynamic>))
          .toList(),
    );
  }

  static final _controller =
      StreamController<UnsignedTransactionCreatedResponse>.broadcast();
  static Stream<UnsignedTransactionCreatedResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(UnsignedTransactionCreatedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class TransactionSignedOfflineResponse {
  final bool success;
  final String? error;
  final String? txId;
  final int fee;
  final String? txBlob;
  final String? txKey;
  final List<String> txKeyAdditional;
  final List<ChangeOutput> changeOutputs;

  TransactionSignedOfflineResponse({
    required this.success,
    this.error,
    this.txId,
    required this.fee,
    this.txBlob,
    this.txKey,
    required this.txKeyAdditional,
    required this.changeOutputs,
  });

  factory TransactionSignedOfflineResponse.fromJson(
      Map<String, dynamic> json) {
    return TransactionSignedOfflineResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      txId: json['tx_id'] as String?,
      fee: json['fee'] as int,
      txBlob: json['tx_blob'] as String?,
      txKey: json['tx_key'] as String?,
      txKeyAdditional:
          (json['tx_key_additional'] as List).cast<String>(),
      changeOutputs: (json['change_outputs'] as List)
          .map((e) => ChangeOutput.fromJson(e as Map<String, dynamic>))
          .toList(),
    );
  }

  static final _controller =
      StreamController<TransactionSignedOfflineResponse>.broadcast();
  static Stream<TransactionSignedOfflineResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(TransactionSignedOfflineResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class KeyImagesExportedResponse {
  final bool success;
  final String? error;
  final String? keyImagesHex;
  final int count;

  KeyImagesExportedResponse({
    required this.success,
    this.error,
    this.keyImagesHex,
    required this.count,
  });

  factory KeyImagesExportedResponse.fromJson(Map<String, dynamic> json) {
    return KeyImagesExportedResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      keyImagesHex: json['key_images_hex'] as String?,
      count: json['count'] as int,
    );
  }

  static final _controller =
      StreamController<KeyImagesExportedResponse>.broadcast();
  static Stream<KeyImagesExportedResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(KeyImagesExportedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}

class KeyImagesImportedResponse {
  final bool success;
  final String? error;
  final int importedCount;
  final int spentCount;

  KeyImagesImportedResponse({
    required this.success,
    this.error,
    required this.importedCount,
    required this.spentCount,
  });

  factory KeyImagesImportedResponse.fromJson(Map<String, dynamic> json) {
    return KeyImagesImportedResponse(
      success: json['success'] as bool,
      error: json['error'] as String?,
      importedCount: json['imported_count'] as int,
      spentCount: json['spent_count'] as int,
    );
  }

  static final _controller =
      StreamController<KeyImagesImportedResponse>.broadcast();
  static Stream<KeyImagesImportedResponse> get stream =>
      _controller.stream;

  static void handleFromRust(String jsonStr) {
    _controller.add(KeyImagesImportedResponse.fromJson(
        jsonDecode(jsonStr) as Map<String, dynamic>));
  }
}
