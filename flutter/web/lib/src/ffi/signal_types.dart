import 'dart:async';
import 'dart:convert';
import 'signal_sender.dart';

export 'signal_sender.dart' show setSignalSender;

// ---------------------------------------------------------------------------
// Sub-structs (shared between multiple signals)
// ---------------------------------------------------------------------------

class Recipient {
  final String address;
  final int amount;

  const Recipient({required this.address, required this.amount});

  Map<String, dynamic> toJson() => {'address': address, 'amount': amount};

  factory Recipient.fromJson(Map<String, dynamic> json) => Recipient(
    address: json['address'] as String,
    amount: json['amount'] as int,
  );
}

class ChangeOutput {
  final String txHash;
  final int outputIndex;
  final int amount;
  final String amountXmr;
  final String key;
  final String keyOffset;
  final String commitmentMask;
  final (int, int)? subaddressIndex;
  final String receivedOutputBytes;
  final String keyImage;

  const ChangeOutput({
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

  Map<String, dynamic> toJson() => {
    'tx_hash': txHash,
    'output_index': outputIndex,
    'amount': amount,
    'amount_xmr': amountXmr,
    'key': key,
    'key_offset': keyOffset,
    'commitment_mask': commitmentMask,
    if (subaddressIndex != null)
      'subaddress_index': [subaddressIndex!.$1, subaddressIndex!.$2],
    'received_output_bytes': receivedOutputBytes,
    'key_image': keyImage,
  };

  factory ChangeOutput.fromJson(Map<String, dynamic> json) {
    final si = json['subaddress_index'];
    return ChangeOutput(
      txHash: json['tx_hash'] as String,
      outputIndex: json['output_index'] as int,
      amount: json['amount'] as int,
      amountXmr: json['amount_xmr'] as String,
      key: json['key'] as String,
      keyOffset: json['key_offset'] as String,
      commitmentMask: json['commitment_mask'] as String,
      subaddressIndex: si != null
          ? ((si as List)[0] as int, (si as List)[1] as int)
          : null,
      receivedOutputBytes: json['received_output_bytes'] as String,
      keyImage: json['key_image'] as String,
    );
  }
}

class OwnedOutput {
  final String txHash;
  final int outputIndex;
  final int amount;
  final String amountXmr;
  final String key;
  final String keyOffset;
  final String commitmentMask;
  final (int, int)? subaddressIndex;
  final String? paymentId;
  final String receivedOutputBytes;
  final int blockHeight;
  final bool spent;
  final String keyImage;
  final bool isCoinbase;
  final bool frozen;

  const OwnedOutput({
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

  Map<String, dynamic> toJson() => {
    'tx_hash': txHash,
    'output_index': outputIndex,
    'amount': amount,
    'amount_xmr': amountXmr,
    'key': key,
    'key_offset': keyOffset,
    'commitment_mask': commitmentMask,
    if (subaddressIndex != null)
      'subaddress_index': [subaddressIndex!.$1, subaddressIndex!.$2],
    if (paymentId != null) 'payment_id': paymentId,
    'received_output_bytes': receivedOutputBytes,
    'block_height': blockHeight,
    'spent': spent,
    'key_image': keyImage,
    'is_coinbase': isCoinbase,
    'frozen': frozen,
  };

  factory OwnedOutput.fromJson(Map<String, dynamic> json) {
    final si = json['subaddress_index'];
    return OwnedOutput(
      txHash: json['tx_hash'] as String,
      outputIndex: json['output_index'] as int,
      amount: json['amount'] as int,
      amountXmr: json['amount_xmr'] as String,
      key: json['key'] as String,
      keyOffset: json['key_offset'] as String,
      commitmentMask: json['commitment_mask'] as String,
      subaddressIndex: si != null
          ? ((si as List)[0] as int, (si as List)[1] as int)
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

  OwnedOutput copyWith({bool? spent, bool? frozen, String? keyImage}) =>
      OwnedOutput(
        txHash: txHash,
        outputIndex: outputIndex,
        amount: amount,
        amountXmr: amountXmr,
        key: key,
        keyOffset: keyOffset,
        commitmentMask: commitmentMask,
        subaddressIndex: subaddressIndex,
        paymentId: paymentId,
        receivedOutputBytes: receivedOutputBytes,
        blockHeight: blockHeight,
        spent: spent ?? this.spent,
        keyImage: keyImage ?? this.keyImage,
        isCoinbase: isCoinbase,
        frozen: frozen ?? this.frozen,
      );
}

class WalletConfig {
  final String seed;
  final String network;
  final int accountLookahead;
  final int subaddressLookahead;
  final List<int>? accountsToScan;
  final String passphrase;
  final int bip39AccountIndex;

  const WalletConfig({
    required this.seed,
    required this.network,
    required this.accountLookahead,
    this.subaddressLookahead = 0,
    this.accountsToScan,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  Map<String, dynamic> toJson() => {
    'seed': seed,
    'network': network,
    'account_lookahead': accountLookahead,
    'subaddress_lookahead': subaddressLookahead,
    if (accountsToScan != null) 'accounts_to_scan': accountsToScan,
    'passphrase': passphrase,
    'bip39_account_index': bip39AccountIndex,
  };
}

class WalletScanResult {
  final String address;
  final List<OwnedOutput> outputs;

  const WalletScanResult({required this.address, required this.outputs});

  Map<String, dynamic> toJson() => {
    'address': address,
    'outputs': outputs.map((e) => e.toJson()).toList(),
  };

  factory WalletScanResult.fromJson(Map<String, dynamic> json) =>
      WalletScanResult(
        address: json['address'] as String,
        outputs: (json['outputs'] as List)
            .map((e) => OwnedOutput.fromJson(e as Map<String, dynamic>))
            .toList(),
      );
}

class DoubleSpendConflict {
  final String keyImage;
  final int previousSpentHeight;
  final int newHeight;

  const DoubleSpendConflict({
    required this.keyImage,
    required this.previousSpentHeight,
    required this.newHeight,
  });

  Map<String, dynamic> toJson() => {
    'key_image': keyImage,
    'previous_spent_height': previousSpentHeight,
    'new_height': newHeight,
  };

  factory DoubleSpendConflict.fromJson(Map<String, dynamic> json) =>
      DoubleSpendConflict(
        keyImage: json['key_image'] as String,
        previousSpentHeight: json['previous_spent_height'] as int,
        newHeight: json['new_height'] as int,
      );
}

// ---------------------------------------------------------------------------
// Helper: send a DartSignal to the worker
// ---------------------------------------------------------------------------

void _send(String fnName, Map<String, dynamic> data) {
  signalSender.sendSignal(fnName, data);
}

// ---------------------------------------------------------------------------
// DartSignal types (Dart -> Rust)
// Each has sendSignalToRust().
// ---------------------------------------------------------------------------

class MoneroTestRequest {
  const MoneroTestRequest();

  void sendSignalToRust() => _send('send_monero_test_request', {});
}

class CreateWalletRequest {
  final String password;
  final String network;
  const CreateWalletRequest({required this.password, required this.network});

  void sendSignalToRust() => _send('send_create_wallet_request', {
    'password': password,
    'network': network,
  });
}

class StartSyncRequest {
  const StartSyncRequest();

  void sendSignalToRust() => _send('send_start_sync_request', {});
}

class GetBalanceRequest {
  const GetBalanceRequest();

  void sendSignalToRust() => _send('send_get_balance_request', {});
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
  const CreateTransactionRequest({
    required this.nodeUrl,
    required this.seed,
    required this.network,
    required this.recipients,
    this.selectedOutputs,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
    this.subtractFee = false,
  });

  void sendSignalToRust() => _send('send_create_transaction_request', {
    'node_url': nodeUrl,
    'seed': seed,
    'network': network,
    'recipients': recipients.map((e) => e.toJson()).toList(),
    if (selectedOutputs != null) 'selected_outputs': selectedOutputs,
    'passphrase': passphrase,
    'bip39_account_index': bip39AccountIndex,
    'subtract_fee': subtractFee,
  });
}

class SweepAllRequest {
  final String nodeUrl;
  final String seed;
  final String network;
  final String destinationAddress;
  final List<String>? selectedOutputs;
  final String passphrase;
  final int bip39AccountIndex;
  const SweepAllRequest({
    required this.nodeUrl,
    required this.seed,
    required this.network,
    required this.destinationAddress,
    this.selectedOutputs,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  void sendSignalToRust() => _send('send_sweep_all_request', {
    'node_url': nodeUrl,
    'seed': seed,
    'network': network,
    'destination_address': destinationAddress,
    if (selectedOutputs != null) 'selected_outputs': selectedOutputs,
    'passphrase': passphrase,
    'bip39_account_index': bip39AccountIndex,
  });
}

class GenerateSeedRequest {
  final String seedType;
  const GenerateSeedRequest({required this.seedType});

  void sendSignalToRust() =>
      _send('send_generate_seed_request', {'seed_type': seedType});
}

class GetSeedBirthdayRequest {
  final String seed;
  final String passphrase;
  final int bip39AccountIndex;
  const GetSeedBirthdayRequest({
    required this.seed,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  void sendSignalToRust() => _send('send_get_seed_birthday_request', {
    'seed': seed,
    'passphrase': passphrase,
    'bip39_account_index': bip39AccountIndex,
  });
}

class GetBlockHeightFromTimestampRequest {
  final int timestamp;
  final String nodeUrl;
  const GetBlockHeightFromTimestampRequest({
    required this.timestamp,
    required this.nodeUrl,
  });

  void sendSignalToRust() => _send(
    'send_get_block_height_from_timestamp_request',
    {'timestamp': timestamp, 'node_url': nodeUrl},
  );
}

class DeriveAddressRequest {
  final String seed;
  final String network;
  final String passphrase;
  final int bip39AccountIndex;
  const DeriveAddressRequest({
    required this.seed,
    required this.network,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  void sendSignalToRust() => _send('send_derive_address_request', {
    'seed': seed,
    'network': network,
    'passphrase': passphrase,
    'bip39_account_index': bip39AccountIndex,
  });
}

class DeriveSubaddressRequest {
  final String seed;
  final String network;
  final int account;
  final int addressIndex;
  final String passphrase;
  final int bip39AccountIndex;
  const DeriveSubaddressRequest({
    required this.seed,
    required this.network,
    required this.account,
    required this.addressIndex,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  void sendSignalToRust() => _send('send_derive_subaddress_request', {
    'seed': seed,
    'network': network,
    'account': account,
    'address_index': addressIndex,
    'passphrase': passphrase,
    'bip39_account_index': bip39AccountIndex,
  });
}

class DeriveKeysRequest {
  final String seed;
  final String network;
  final String passphrase;
  final int bip39AccountIndex;
  const DeriveKeysRequest({
    required this.seed,
    required this.network,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  void sendSignalToRust() => _send('send_derive_keys_request', {
    'seed': seed,
    'network': network,
    'passphrase': passphrase,
    'bip39_account_index': bip39AccountIndex,
  });
}

class ScanBlockRequest {
  final String nodeUrl;
  final int blockHeight;
  final String seed;
  final String network;
  final String passphrase;
  final int bip39AccountIndex;
  const ScanBlockRequest({
    required this.nodeUrl,
    required this.blockHeight,
    required this.seed,
    required this.network,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  void sendSignalToRust() => _send('send_scan_block_request', {
    'node_url': nodeUrl,
    'block_height': blockHeight,
    'seed': seed,
    'network': network,
    'passphrase': passphrase,
    'bip39_account_index': bip39AccountIndex,
  });
}

class BroadcastTransactionRequest {
  final String nodeUrl;
  final String txBlob;
  final List<String> spentOutputHashes;
  final String txId;
  final List<String> spentKeyImages;
  final bool doNotRelay;
  const BroadcastTransactionRequest({
    required this.nodeUrl,
    required this.txBlob,
    required this.spentOutputHashes,
    this.txId = '',
    this.spentKeyImages = const [],
    this.doNotRelay = false,
  });

  void sendSignalToRust() => _send('send_broadcast_transaction_request', {
    'node_url': nodeUrl,
    'tx_blob': txBlob,
    'spent_output_hashes': spentOutputHashes,
    'tx_id': txId,
    'spent_key_images': spentKeyImages,
    'do_not_relay': doNotRelay,
  });
}

class QueryDaemonHeightRequest {
  final String nodeUrl;
  const QueryDaemonHeightRequest({required this.nodeUrl});

  void sendSignalToRust() =>
      _send('send_query_daemon_height_request', {'node_url': nodeUrl});
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
  const StartContinuousScanRequest({
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

  void sendSignalToRust() => _send('send_start_continuous_scan_request', {
    'node_url': nodeUrl,
    'start_height': startHeight,
    'seed': seed,
    'network': network,
    'account_lookahead': accountLookahead,
    'subaddress_lookahead': subaddressLookahead,
    if (accountsToScan != null) 'accounts_to_scan': accountsToScan,
    'passphrase': passphrase,
    'bip39_account_index': bip39AccountIndex,
  });
}

class StopScanRequest {
  const StopScanRequest();

  void sendSignalToRust() => _send('send_stop_scan_request', {});
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
  const MempoolScanRequest({
    required this.nodeUrl,
    required this.seed,
    required this.network,
    required this.accountLookahead,
    this.subaddressLookahead = 0,
    this.accountsToScan,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  void sendSignalToRust() => _send('send_mempool_scan_request', {
    'node_url': nodeUrl,
    'seed': seed,
    'network': network,
    'account_lookahead': accountLookahead,
    'subaddress_lookahead': subaddressLookahead,
    if (accountsToScan != null) 'accounts_to_scan': accountsToScan,
    'passphrase': passphrase,
    'bip39_account_index': bip39AccountIndex,
  });
}

class GenerateOutProofRequest {
  final String txId;
  final String txKey;
  final String recipientAddress;
  final String message;
  final String network;
  const GenerateOutProofRequest({
    required this.txId,
    required this.txKey,
    required this.recipientAddress,
    required this.message,
    required this.network,
  });

  void sendSignalToRust() => _send('send_generate_out_proof_request', {
    'tx_id': txId,
    'tx_key': txKey,
    'recipient_address': recipientAddress,
    'message': message,
    'network': network,
  });
}

class SaveWalletDataRequest {
  final String password;
  final String walletDataJson;
  const SaveWalletDataRequest({
    required this.password,
    required this.walletDataJson,
  });

  void sendSignalToRust() => _send('send_save_wallet_data_request', {
    'password': password,
    'wallet_data_json': walletDataJson,
  });
}

class LoadWalletDataRequest {
  final String password;
  final String encryptedData;
  const LoadWalletDataRequest({
    required this.password,
    required this.encryptedData,
  });

  void sendSignalToRust() => _send('send_load_wallet_data_request', {
    'password': password,
    'encrypted_data': encryptedData,
  });
}

class DeriveEncryptionKeyRequest {
  final String password;
  const DeriveEncryptionKeyRequest({required this.password});

  void sendSignalToRust() =>
      _send('send_derive_encryption_key_request', {'password': password});
}

class SaveWithDerivedKeyRequest {
  final String keyHex;
  final String saltHex;
  final String walletDataJson;
  const SaveWithDerivedKeyRequest({
    required this.keyHex,
    required this.saltHex,
    required this.walletDataJson,
  });

  void sendSignalToRust() => _send('send_save_with_derived_key_request', {
    'key_hex': keyHex,
    'salt_hex': saltHex,
    'wallet_data_json': walletDataJson,
  });
}

class ScanBlockMultiWalletRequest {
  final String nodeUrl;
  final int blockHeight;
  final List<WalletConfig> wallets;
  const ScanBlockMultiWalletRequest({
    required this.nodeUrl,
    required this.blockHeight,
    required this.wallets,
  });

  void sendSignalToRust() => _send('send_scan_block_multi_wallet_request', {
    'node_url': nodeUrl,
    'block_height': blockHeight,
    'wallets': wallets.map((e) => e.toJson()).toList(),
  });
}

class StartMultiWalletScanRequest {
  final String nodeUrl;
  final int startHeight;
  final List<WalletConfig> wallets;
  const StartMultiWalletScanRequest({
    required this.nodeUrl,
    required this.startHeight,
    required this.wallets,
  });

  void sendSignalToRust() => _send('send_start_multi_wallet_scan_request', {
    'node_url': nodeUrl,
    'start_height': startHeight,
    'wallets': wallets.map((e) => e.toJson()).toList(),
  });
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
  const RestoreWalletDataRequest({
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

  void sendSignalToRust() => _send('send_restore_wallet_data_request', {
    'seed': seed,
    'network': network,
    'outputs': outputs.map((e) => e.toJson()).toList(),
    'daemon_height': daemonHeight,
    'current_height': currentHeight,
    if (blockHashesJson != null) 'block_hashes_json': blockHashesJson,
    if (pendingStateJson != null) 'pending_state_json': pendingStateJson,
    'passphrase': passphrase,
    'bip39_account_index': bip39AccountIndex,
  });
}

class GetBlockHashesRequest {
  const GetBlockHashesRequest();

  void sendSignalToRust() => _send('send_get_block_hashes_request', {});
}

class GetPendingStateRequest {
  const GetPendingStateRequest();

  void sendSignalToRust() => _send('send_get_pending_state_request', {});
}

class ConvertBip39ToLegacyRequest {
  final String bip39Mnemonic;
  final int accountIndex;
  final String passphrase;
  const ConvertBip39ToLegacyRequest({
    required this.bip39Mnemonic,
    required this.accountIndex,
    this.passphrase = '',
  });

  void sendSignalToRust() => _send('send_convert_bip39_to_legacy_request', {
    'bip39_mnemonic': bip39Mnemonic,
    'account_index': accountIndex,
    'passphrase': passphrase,
  });
}

class FreezeOutputRequest {
  final String keyImage;
  const FreezeOutputRequest({required this.keyImage});

  void sendSignalToRust() =>
      _send('send_freeze_output_request', {'key_image': keyImage});
}

class ThawOutputRequest {
  final String keyImage;
  const ThawOutputRequest({required this.keyImage});

  void sendSignalToRust() =>
      _send('send_thaw_output_request', {'key_image': keyImage});
}

class CreateUnsignedTransactionRequest {
  final String nodeUrl;
  final String viewKeyHex;
  final String pubSpendKeyHex;
  final String network;
  final List<Recipient> recipients;
  final List<String>? selectedOutputs;
  const CreateUnsignedTransactionRequest({
    required this.nodeUrl,
    required this.viewKeyHex,
    required this.pubSpendKeyHex,
    required this.network,
    required this.recipients,
    this.selectedOutputs,
  });

  void sendSignalToRust() => _send('send_create_unsigned_transaction_request', {
    'node_url': nodeUrl,
    'view_key_hex': viewKeyHex,
    'pub_spend_key_hex': pubSpendKeyHex,
    'network': network,
    'recipients': recipients.map((e) => e.toJson()).toList(),
    if (selectedOutputs != null) 'selected_outputs': selectedOutputs,
  });
}

class SignUnsignedTransactionRequest {
  final String seed;
  final String unsignedTxHex;
  final String network;
  final String passphrase;
  final int bip39AccountIndex;
  final String? spendSecretKeyHex;
  final String? viewSecretKeyHex;
  const SignUnsignedTransactionRequest({
    required this.seed,
    required this.unsignedTxHex,
    required this.network,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
    this.spendSecretKeyHex,
    this.viewSecretKeyHex,
  });

  void sendSignalToRust() => _send('send_sign_unsigned_transaction_request', {
    'seed': seed,
    'unsigned_tx_hex': unsignedTxHex,
    'network': network,
    'passphrase': passphrase,
    'bip39_account_index': bip39AccountIndex,
    if (spendSecretKeyHex != null) 'spend_secret_key_hex': spendSecretKeyHex,
    if (viewSecretKeyHex != null) 'view_secret_key_hex': viewSecretKeyHex,
  });
}

class ExtractSignedTxSetRequest {
  final String dataHex;
  final String viewKeyHex;

  const ExtractSignedTxSetRequest({
    required this.dataHex,
    required this.viewKeyHex,
  });

  void sendSignalToRust() => _send('send_extract_signed_txset_request', {
    'data_hex': dataHex,
    'view_key_hex': viewKeyHex,
  });
}

class InspectUnsignedTxSetRequest {
  final String dataHex;
  final String viewKeyHex;

  const InspectUnsignedTxSetRequest({
    required this.dataHex,
    required this.viewKeyHex,
  });

  void sendSignalToRust() => _send('send_inspect_unsigned_txset_request', {
    'data_hex': dataHex,
    'view_key_hex': viewKeyHex,
  });
}

class ExportKeyImagesRequest {
  final String seed;
  final String network;
  final String passphrase;
  final int bip39AccountIndex;
  const ExportKeyImagesRequest({
    required this.seed,
    required this.network,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  void sendSignalToRust() => _send('send_export_key_images_request', {
    'seed': seed,
    'network': network,
    'passphrase': passphrase,
    'bip39_account_index': bip39AccountIndex,
  });
}

class ImportKeyImagesRequest {
  final String dataHex;
  final String nodeUrl;
  final String seed;
  final String passphrase;
  final int bip39AccountIndex;
  const ImportKeyImagesRequest({
    required this.dataHex,
    required this.nodeUrl,
    required this.seed,
    this.passphrase = '',
    this.bip39AccountIndex = 0,
  });

  void sendSignalToRust() => _send('send_import_key_images_request', {
    'data_hex': dataHex,
    'node_url': nodeUrl,
    'seed': seed,
    'passphrase': passphrase,
    'bip39_account_index': bip39AccountIndex,
  });
}

class ImportKeysFileRequest {
  final String fileBytesHex;
  final String password;
  const ImportKeysFileRequest({
    required this.fileBytesHex,
    required this.password,
  });

  void sendSignalToRust() => _send('send_import_keys_file_request', {
    'file_bytes_hex': fileBytesHex,
    'password': password,
  });
}

class ExportKeysFileRequest {
  final String seed;
  final String network;
  final String password;
  const ExportKeysFileRequest({
    required this.seed,
    required this.network,
    required this.password,
  });

  void sendSignalToRust() => _send('send_export_keys_file_request', {
    'seed': seed,
    'network': network,
    'password': password,
  });
}

class StartUrEncoderRequest {
  final String dataHex;
  final String urType;
  final int maxFragmentLen;
  const StartUrEncoderRequest({
    required this.dataHex,
    required this.urType,
    required this.maxFragmentLen,
  });

  void sendSignalToRust() => _send('send_start_ur_encoder_request', {
    'data_hex': dataHex,
    'ur_type': urType,
    'max_fragment_len': maxFragmentLen,
  });
}

class StopUrEncoderRequest {
  const StopUrEncoderRequest();

  void sendSignalToRust() => _send('send_stop_ur_encoder_request', {});
}

class UrDecodeFrameRequest {
  final String uri;
  const UrDecodeFrameRequest({required this.uri});

  void sendSignalToRust() =>
      _send('send_ur_decode_frame_request', {'uri': uri});
}

class ResetUrDecoderRequest {
  const ResetUrDecoderRequest();

  void sendSignalToRust() => _send('send_reset_ur_decoder_request', {});
}

// ---------------------------------------------------------------------------
// RustSignal types (Rust -> Dart, 37 total)
// Each has fromJson() and a static stream getter.
// ---------------------------------------------------------------------------

class MoneroTestResponse {
  final String result;

  const MoneroTestResponse({required this.result});

  factory MoneroTestResponse.fromJson(Map<String, dynamic> json) =>
      MoneroTestResponse(result: json['result'] as String);

  static Stream<MoneroTestResponse> get stream => signalSender
      .onRawSignal('MoneroTestResponse')
      .map(MoneroTestResponse.fromJson);
}

class WalletCreatedResponse {
  final String address;

  const WalletCreatedResponse({required this.address});

  factory WalletCreatedResponse.fromJson(Map<String, dynamic> json) =>
      WalletCreatedResponse(address: json['address'] as String);

  static Stream<WalletCreatedResponse> get stream => signalSender
      .onRawSignal('WalletCreatedResponse')
      .map(WalletCreatedResponse.fromJson);
}

class SyncProgressResponse {
  final int currentHeight;
  final int daemonHeight;
  final bool isSynced;
  final bool isScanning;

  const SyncProgressResponse({
    required this.currentHeight,
    required this.daemonHeight,
    required this.isSynced,
    required this.isScanning,
  });

  factory SyncProgressResponse.fromJson(Map<String, dynamic> json) =>
      SyncProgressResponse(
        currentHeight: json['current_height'] as int,
        daemonHeight: json['daemon_height'] as int,
        isSynced: json['is_synced'] as bool,
        isScanning: json['is_scanning'] as bool,
      );

  static Stream<SyncProgressResponse> get stream => signalSender
      .onRawSignal('SyncProgressResponse')
      .map(SyncProgressResponse.fromJson);
}

class BalanceResponse {
  final int confirmed;
  final int unconfirmed;
  final int pendingSpend;

  const BalanceResponse({
    required this.confirmed,
    required this.unconfirmed,
    required this.pendingSpend,
  });

  factory BalanceResponse.fromJson(Map<String, dynamic> json) =>
      BalanceResponse(
        confirmed: json['confirmed'] as int,
        unconfirmed: json['unconfirmed'] as int,
        pendingSpend: json['pending_spend'] as int,
      );

  static Stream<BalanceResponse> get stream =>
      signalSender.onRawSignal('BalanceResponse').map(BalanceResponse.fromJson);
}

class TransactionCreatedResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final String txId;
  final int fee;
  final String? txBlob;
  final String? txKey;
  final List<String> txKeyAdditional;
  final List<String> spentOutputHashes;
  final List<ChangeOutput> changeOutputs;

  const TransactionCreatedResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    required this.txId,
    required this.fee,
    this.txBlob,
    this.txKey,
    required this.txKeyAdditional,
    required this.spentOutputHashes,
    required this.changeOutputs,
  });

  factory TransactionCreatedResponse.fromJson(Map<String, dynamic> json) =>
      TransactionCreatedResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        txId: json['tx_id'] as String,
        fee: json['fee'] as int,
        txBlob: json['tx_blob'] as String?,
        txKey: json['tx_key'] as String?,
        txKeyAdditional: (json['tx_key_additional'] as List).cast<String>(),
        spentOutputHashes: (json['spent_output_hashes'] as List).cast<String>(),
        changeOutputs: (json['change_outputs'] as List)
            .map((e) => ChangeOutput.fromJson(e as Map<String, dynamic>))
            .toList(),
      );

  static Stream<TransactionCreatedResponse> get stream => signalSender
      .onRawSignal('TransactionCreatedResponse')
      .map(TransactionCreatedResponse.fromJson);
}

class SeedGeneratedResponse {
  final String seed;
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final int? restoreHeight;

  const SeedGeneratedResponse({
    required this.seed,
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    this.restoreHeight,
  });

  factory SeedGeneratedResponse.fromJson(Map<String, dynamic> json) =>
      SeedGeneratedResponse(
        seed: json['seed'] as String,
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        restoreHeight: json['restore_height'] as int?,
      );

  static Stream<SeedGeneratedResponse> get stream => signalSender
      .onRawSignal('SeedGeneratedResponse')
      .map(SeedGeneratedResponse.fromJson);
}

class SeedBirthdayResponse {
  final int? birthday;
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;

  const SeedBirthdayResponse({
    this.birthday,
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
  });

  factory SeedBirthdayResponse.fromJson(Map<String, dynamic> json) =>
      SeedBirthdayResponse(
        birthday: json['birthday'] as int?,
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
      );

  static Stream<SeedBirthdayResponse> get stream => signalSender
      .onRawSignal('SeedBirthdayResponse')
      .map(SeedBirthdayResponse.fromJson);
}

class BlockHeightFromTimestampResponse {
  final int blockHeight;
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;

  const BlockHeightFromTimestampResponse({
    required this.blockHeight,
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
  });

  factory BlockHeightFromTimestampResponse.fromJson(
    Map<String, dynamic> json,
  ) => BlockHeightFromTimestampResponse(
    blockHeight: json['block_height'] as int,
    success: json['success'] as bool,
    error: json['error'] as String?,
    errorCode: json['error_code'] as int?,
    errorHint: json['error_hint'] as String?,
    errorTransient: json['error_transient'] as bool?,
  );

  static Stream<BlockHeightFromTimestampResponse> get stream => signalSender
      .onRawSignal('BlockHeightFromTimestampResponse')
      .map(BlockHeightFromTimestampResponse.fromJson);
}

class AddressDerivedResponse {
  final String address;
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;

  const AddressDerivedResponse({
    required this.address,
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
  });

  factory AddressDerivedResponse.fromJson(Map<String, dynamic> json) =>
      AddressDerivedResponse(
        address: json['address'] as String,
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
      );

  static Stream<AddressDerivedResponse> get stream => signalSender
      .onRawSignal('AddressDerivedResponse')
      .map(AddressDerivedResponse.fromJson);
}

class SubaddressDerivedResponse {
  final String address;
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;

  const SubaddressDerivedResponse({
    required this.address,
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
  });

  factory SubaddressDerivedResponse.fromJson(Map<String, dynamic> json) =>
      SubaddressDerivedResponse(
        address: json['address'] as String,
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
      );

  static Stream<SubaddressDerivedResponse> get stream => signalSender
      .onRawSignal('SubaddressDerivedResponse')
      .map(SubaddressDerivedResponse.fromJson);
}

class KeysDerivedResponse {
  final String address;
  final String secretSpendKey;
  final String secretViewKey;
  final String publicSpendKey;
  final String publicViewKey;
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;

  const KeysDerivedResponse({
    required this.address,
    required this.secretSpendKey,
    required this.secretViewKey,
    required this.publicSpendKey,
    required this.publicViewKey,
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
  });

  factory KeysDerivedResponse.fromJson(Map<String, dynamic> json) =>
      KeysDerivedResponse(
        address: json['address'] as String,
        secretSpendKey: json['secret_spend_key'] as String,
        secretViewKey: json['secret_view_key'] as String,
        publicSpendKey: json['public_spend_key'] as String,
        publicViewKey: json['public_view_key'] as String,
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
      );

  static Stream<KeysDerivedResponse> get stream => signalSender
      .onRawSignal('KeysDerivedResponse')
      .map(KeysDerivedResponse.fromJson);
}

class BlockScanResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final int blockHeight;
  final String blockHash;
  final int blockTimestamp;
  final int txCount;
  final List<OwnedOutput> outputs;
  final int daemonHeight;
  final List<String> spentKeyImages;
  final List<String> spentKeyImageTxHashes;

  const BlockScanResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    required this.blockHeight,
    required this.blockHash,
    required this.blockTimestamp,
    required this.txCount,
    required this.outputs,
    required this.daemonHeight,
    required this.spentKeyImages,
    required this.spentKeyImageTxHashes,
  });

  factory BlockScanResponse.fromJson(Map<String, dynamic> json) =>
      BlockScanResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        blockHeight: json['block_height'] as int,
        blockHash: json['block_hash'] as String,
        blockTimestamp: json['block_timestamp'] as int,
        txCount: json['tx_count'] as int,
        outputs: (json['outputs'] as List)
            .map((e) => OwnedOutput.fromJson(e as Map<String, dynamic>))
            .toList(),
        daemonHeight: json['daemon_height'] as int,
        spentKeyImages: (json['spent_key_images'] as List).cast<String>(),
        spentKeyImageTxHashes: (json['spent_key_image_tx_hashes'] as List)
            .cast<String>(),
      );

  static Stream<BlockScanResponse> get stream => signalSender
      .onRawSignal('BlockScanResponse')
      .map(BlockScanResponse.fromJson);
}

class TransactionBroadcastResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final String? txId;
  final bool isRetryable;
  final bool isDoubleSpend;

  const TransactionBroadcastResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    this.txId,
    this.isRetryable = false,
    this.isDoubleSpend = false,
  });

  factory TransactionBroadcastResponse.fromJson(Map<String, dynamic> json) =>
      TransactionBroadcastResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        txId: json['tx_id'] as String?,
        isRetryable: json['is_retryable'] as bool? ?? false,
        isDoubleSpend: json['is_double_spend'] as bool? ?? false,
      );

  static Stream<TransactionBroadcastResponse> get stream => signalSender
      .onRawSignal('TransactionBroadcastResponse')
      .map(TransactionBroadcastResponse.fromJson);
}

class DaemonHeightResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final int daemonHeight;

  const DaemonHeightResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    required this.daemonHeight,
  });

  factory DaemonHeightResponse.fromJson(Map<String, dynamic> json) =>
      DaemonHeightResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        daemonHeight: json['daemon_height'] as int,
      );

  static Stream<DaemonHeightResponse> get stream => signalSender
      .onRawSignal('DaemonHeightResponse')
      .map(DaemonHeightResponse.fromJson);
}

class SpentStatusUpdatedResponse {
  final List<String> spentKeyImages;

  const SpentStatusUpdatedResponse({required this.spentKeyImages});

  factory SpentStatusUpdatedResponse.fromJson(Map<String, dynamic> json) =>
      SpentStatusUpdatedResponse(
        spentKeyImages: (json['spent_key_images'] as List).cast<String>(),
      );

  static Stream<SpentStatusUpdatedResponse> get stream => signalSender
      .onRawSignal('SpentStatusUpdatedResponse')
      .map(SpentStatusUpdatedResponse.fromJson);
}

class MempoolScanResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final int txCount;
  final List<OwnedOutput> outputs;
  final List<String> spentKeyImages;
  final List<String> spentKeyImageTxHashes;

  const MempoolScanResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    required this.txCount,
    required this.outputs,
    required this.spentKeyImages,
    required this.spentKeyImageTxHashes,
  });

  factory MempoolScanResponse.fromJson(Map<String, dynamic> json) =>
      MempoolScanResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        txCount: json['tx_count'] as int,
        outputs: (json['outputs'] as List)
            .map((e) => OwnedOutput.fromJson(e as Map<String, dynamic>))
            .toList(),
        spentKeyImages: (json['spent_key_images'] as List).cast<String>(),
        spentKeyImageTxHashes: (json['spent_key_image_tx_hashes'] as List)
            .cast<String>(),
      );

  static Stream<MempoolScanResponse> get stream => signalSender
      .onRawSignal('MempoolScanResponse')
      .map(MempoolScanResponse.fromJson);
}

class OutProofGeneratedResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final String? signature;
  final String? formatted;

  const OutProofGeneratedResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    this.signature,
    this.formatted,
  });

  factory OutProofGeneratedResponse.fromJson(Map<String, dynamic> json) =>
      OutProofGeneratedResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        signature: json['signature'] as String?,
        formatted: json['formatted'] as String?,
      );

  static Stream<OutProofGeneratedResponse> get stream => signalSender
      .onRawSignal('OutProofGeneratedResponse')
      .map(OutProofGeneratedResponse.fromJson);
}

class WalletDataSavedResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final String? encryptedData;

  const WalletDataSavedResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    this.encryptedData,
  });

  factory WalletDataSavedResponse.fromJson(Map<String, dynamic> json) =>
      WalletDataSavedResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        encryptedData: json['encrypted_data'] as String?,
      );

  static Stream<WalletDataSavedResponse> get stream => signalSender
      .onRawSignal('WalletDataSavedResponse')
      .map(WalletDataSavedResponse.fromJson);
}

class WalletDataLoadedResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final String? walletDataJson;

  const WalletDataLoadedResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    this.walletDataJson,
  });

  factory WalletDataLoadedResponse.fromJson(Map<String, dynamic> json) =>
      WalletDataLoadedResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        walletDataJson: json['wallet_data_json'] as String?,
      );

  static Stream<WalletDataLoadedResponse> get stream => signalSender
      .onRawSignal('WalletDataLoadedResponse')
      .map(WalletDataLoadedResponse.fromJson);
}

class EncryptionKeyDerivedResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final String? keyHex;
  final String? saltHex;

  const EncryptionKeyDerivedResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    this.keyHex,
    this.saltHex,
  });

  factory EncryptionKeyDerivedResponse.fromJson(Map<String, dynamic> json) =>
      EncryptionKeyDerivedResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        keyHex: json['key_hex'] as String?,
        saltHex: json['salt_hex'] as String?,
      );

  static Stream<EncryptionKeyDerivedResponse> get stream => signalSender
      .onRawSignal('EncryptionKeyDerivedResponse')
      .map(EncryptionKeyDerivedResponse.fromJson);
}

class MultiWalletScanResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final int blockHeight;
  final String blockHash;
  final int blockTimestamp;
  final int txCount;
  final int daemonHeight;
  final List<String> spentKeyImages;
  final List<String> spentKeyImageTxHashes;
  final List<WalletScanResult> walletResults;

  const MultiWalletScanResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    required this.blockHeight,
    required this.blockHash,
    required this.blockTimestamp,
    required this.txCount,
    required this.daemonHeight,
    required this.spentKeyImages,
    required this.spentKeyImageTxHashes,
    required this.walletResults,
  });

  factory MultiWalletScanResponse.fromJson(Map<String, dynamic> json) =>
      MultiWalletScanResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        blockHeight: json['block_height'] as int,
        blockHash: json['block_hash'] as String,
        blockTimestamp: json['block_timestamp'] as int,
        txCount: json['tx_count'] as int,
        daemonHeight: json['daemon_height'] as int,
        spentKeyImages: (json['spent_key_images'] as List).cast<String>(),
        spentKeyImageTxHashes: (json['spent_key_image_tx_hashes'] as List)
            .cast<String>(),
        walletResults: (json['wallet_results'] as List)
            .map((e) => WalletScanResult.fromJson(e as Map<String, dynamic>))
            .toList(),
      );

  static Stream<MultiWalletScanResponse> get stream => signalSender
      .onRawSignal('MultiWalletScanResponse')
      .map(MultiWalletScanResponse.fromJson);
}

class BlockHashesResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final String? blockHashesJson;

  const BlockHashesResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    this.blockHashesJson,
  });

  factory BlockHashesResponse.fromJson(Map<String, dynamic> json) =>
      BlockHashesResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        blockHashesJson: json['block_hashes_json'] as String?,
      );

  static Stream<BlockHashesResponse> get stream => signalSender
      .onRawSignal('BlockHashesResponse')
      .map(BlockHashesResponse.fromJson);
}

class PendingStateResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final String? pendingStateJson;

  const PendingStateResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    this.pendingStateJson,
  });

  factory PendingStateResponse.fromJson(Map<String, dynamic> json) =>
      PendingStateResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        pendingStateJson: json['pending_state_json'] as String?,
      );

  static Stream<PendingStateResponse> get stream => signalSender
      .onRawSignal('PendingStateResponse')
      .map(PendingStateResponse.fromJson);
}

class ReorgDetectedResponse {
  final int splitHeight;
  final int blocksDetached;
  final int outputsRemoved;
  final int outputsUnspent;
  final List<String> removedKeyImages;
  final List<String> unspentKeyImages;

  const ReorgDetectedResponse({
    required this.splitHeight,
    required this.blocksDetached,
    required this.outputsRemoved,
    required this.outputsUnspent,
    required this.removedKeyImages,
    required this.unspentKeyImages,
  });

  factory ReorgDetectedResponse.fromJson(Map<String, dynamic> json) =>
      ReorgDetectedResponse(
        splitHeight: json['split_height'] as int,
        blocksDetached: json['blocks_detached'] as int,
        outputsRemoved: json['outputs_removed'] as int,
        outputsUnspent: json['outputs_unspent'] as int,
        removedKeyImages: (json['removed_key_images'] as List).cast<String>(),
        unspentKeyImages: (json['unspent_key_images'] as List).cast<String>(),
      );

  static Stream<ReorgDetectedResponse> get stream => signalSender
      .onRawSignal('ReorgDetectedResponse')
      .map(ReorgDetectedResponse.fromJson);
}

class DoubleSpendDetectedResponse {
  final List<DoubleSpendConflict> conflicts;

  const DoubleSpendDetectedResponse({required this.conflicts});

  factory DoubleSpendDetectedResponse.fromJson(Map<String, dynamic> json) =>
      DoubleSpendDetectedResponse(
        conflicts: (json['conflicts'] as List)
            .map((e) => DoubleSpendConflict.fromJson(e as Map<String, dynamic>))
            .toList(),
      );

  static Stream<DoubleSpendDetectedResponse> get stream => signalSender
      .onRawSignal('DoubleSpendDetectedResponse')
      .map(DoubleSpendDetectedResponse.fromJson);
}

class Bip39LegacySeedResponse {
  final String legacySeed;
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;

  const Bip39LegacySeedResponse({
    required this.legacySeed,
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
  });

  factory Bip39LegacySeedResponse.fromJson(Map<String, dynamic> json) =>
      Bip39LegacySeedResponse(
        legacySeed: json['legacy_seed'] as String,
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
      );

  static Stream<Bip39LegacySeedResponse> get stream => signalSender
      .onRawSignal('Bip39LegacySeedResponse')
      .map(Bip39LegacySeedResponse.fromJson);
}

class FreezeThawResponse {
  final bool success;
  final String keyImage;
  final bool frozen;

  const FreezeThawResponse({
    required this.success,
    required this.keyImage,
    required this.frozen,
  });

  factory FreezeThawResponse.fromJson(Map<String, dynamic> json) =>
      FreezeThawResponse(
        success: json['success'] as bool,
        keyImage: json['key_image'] as String,
        frozen: json['frozen'] as bool,
      );

  static Stream<FreezeThawResponse> get stream => signalSender
      .onRawSignal('FreezeThawResponse')
      .map(FreezeThawResponse.fromJson);
}

class TransactionStatusUpdate {
  final String txId;
  final String status;
  final int? confirmedHeight;

  const TransactionStatusUpdate({
    required this.txId,
    required this.status,
    this.confirmedHeight,
  });

  factory TransactionStatusUpdate.fromJson(Map<String, dynamic> json) =>
      TransactionStatusUpdate(
        txId: json['tx_id'] as String,
        status: json['status'] as String,
        confirmedHeight: json['confirmed_height'] as int?,
      );

  static Stream<TransactionStatusUpdate> get stream => signalSender
      .onRawSignal('TransactionStatusUpdate')
      .map(TransactionStatusUpdate.fromJson);
}

class UnsignedTransactionCreatedResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final String? unsignedTxHex;
  final int fee;
  final List<Recipient> recipients;

  const UnsignedTransactionCreatedResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    this.unsignedTxHex,
    required this.fee,
    required this.recipients,
  });

  factory UnsignedTransactionCreatedResponse.fromJson(
    Map<String, dynamic> json,
  ) => UnsignedTransactionCreatedResponse(
    success: json['success'] as bool,
    error: json['error'] as String?,
    errorCode: json['error_code'] as int?,
    errorHint: json['error_hint'] as String?,
    errorTransient: json['error_transient'] as bool?,
    unsignedTxHex: json['unsigned_tx_hex'] as String?,
    fee: json['fee'] as int,
    recipients: (json['recipients'] as List)
        .map((e) => Recipient.fromJson(e as Map<String, dynamic>))
        .toList(),
  );

  static Stream<UnsignedTransactionCreatedResponse> get stream => signalSender
      .onRawSignal('UnsignedTransactionCreatedResponse')
      .map(UnsignedTransactionCreatedResponse.fromJson);
}

class TransactionSignedOfflineResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final String? txId;
  final int fee;
  final String? txBlob;
  final String? txKey;
  final List<String> txKeyAdditional;
  final List<ChangeOutput> changeOutputs;
  final List<String> spentKeyImages;

  const TransactionSignedOfflineResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    this.txId,
    required this.fee,
    this.txBlob,
    this.txKey,
    required this.txKeyAdditional,
    required this.changeOutputs,
    this.spentKeyImages = const [],
  });

  factory TransactionSignedOfflineResponse.fromJson(
    Map<String, dynamic> json,
  ) => TransactionSignedOfflineResponse(
    success: json['success'] as bool,
    error: json['error'] as String?,
    errorCode: json['error_code'] as int?,
    errorHint: json['error_hint'] as String?,
    errorTransient: json['error_transient'] as bool?,
    txId: json['tx_id'] as String?,
    fee: json['fee'] as int,
    txBlob: json['tx_blob'] as String?,
    txKey: json['tx_key'] as String?,
    txKeyAdditional: (json['tx_key_additional'] as List).cast<String>(),
    changeOutputs: (json['change_outputs'] as List)
        .map((e) => ChangeOutput.fromJson(e as Map<String, dynamic>))
        .toList(),
    spentKeyImages: ((json['spent_key_images'] as List?) ?? const [])
        .cast<String>(),
  );

  static Stream<TransactionSignedOfflineResponse> get stream => signalSender
      .onRawSignal('TransactionSignedOfflineResponse')
      .map(TransactionSignedOfflineResponse.fromJson);
}

class ExtractedSignedTransaction {
  final String txId;
  final String txBlob;
  final int txVersion;
  final int txUnlockTime;
  final int txInputCount;
  final List<int> txInputRingSizes;
  final int txOutputCount;
  final int txExtraLen;
  final int? rctType;
  final int? rctFee;
  final int dust;
  final int fee;
  final bool dustAddedToFee;
  final int changeAmount;
  final int selectedTransferCount;
  final List<int> selectedTransferIndices;
  final int keyImagesLen;
  final String keyImagesBlobHex;
  final bool txKeyIsZero;
  final String? txKey;
  final int additionalTxKeyCount;
  final List<String> txKeyAdditional;
  final int destinationCount;
  final int destinationTotalAmount;
  final int multisigSigCount;

  const ExtractedSignedTransaction({
    required this.txId,
    required this.txBlob,
    this.txVersion = 0,
    this.txUnlockTime = 0,
    this.txInputCount = 0,
    this.txInputRingSizes = const [],
    this.txOutputCount = 0,
    this.txExtraLen = 0,
    this.rctType,
    this.rctFee,
    this.dust = 0,
    this.fee = 0,
    this.dustAddedToFee = false,
    this.changeAmount = 0,
    this.selectedTransferCount = 0,
    this.selectedTransferIndices = const [],
    this.keyImagesLen = 0,
    this.keyImagesBlobHex = '',
    this.txKeyIsZero = false,
    this.txKey,
    this.additionalTxKeyCount = 0,
    this.txKeyAdditional = const [],
    this.destinationCount = 0,
    this.destinationTotalAmount = 0,
    this.multisigSigCount = 0,
  });

  factory ExtractedSignedTransaction.fromJson(Map<String, dynamic> json) =>
      ExtractedSignedTransaction(
        txId: json['tx_id'] as String,
        txBlob: json['tx_blob'] as String,
        txVersion: (json['tx_version'] as int?) ?? 0,
        txUnlockTime: (json['tx_unlock_time'] as int?) ?? 0,
        txInputCount: (json['tx_input_count'] as int?) ?? 0,
        txInputRingSizes: ((json['tx_input_ring_sizes'] as List?) ?? const [])
            .cast<int>(),
        txOutputCount: (json['tx_output_count'] as int?) ?? 0,
        txExtraLen: (json['tx_extra_len'] as int?) ?? 0,
        rctType: json['rct_type'] as int?,
        rctFee: json['rct_fee'] as int?,
        dust: (json['dust'] as int?) ?? 0,
        fee: (json['fee'] as int?) ?? 0,
        dustAddedToFee: (json['dust_added_to_fee'] as bool?) ?? false,
        changeAmount: (json['change_amount'] as int?) ?? 0,
        selectedTransferCount: (json['selected_transfer_count'] as int?) ?? 0,
        selectedTransferIndices:
            ((json['selected_transfer_indices'] as List?) ?? const [])
                .cast<int>(),
        keyImagesLen: (json['key_images_len'] as int?) ?? 0,
        keyImagesBlobHex: (json['key_images_blob_hex'] as String?) ?? '',
        txKeyIsZero: (json['tx_key_is_zero'] as bool?) ?? false,
        txKey: json['tx_key'] as String?,
        additionalTxKeyCount: (json['additional_tx_key_count'] as int?) ?? 0,
        txKeyAdditional: ((json['tx_key_additional'] as List?) ?? const [])
            .cast<String>(),
        destinationCount: (json['destination_count'] as int?) ?? 0,
        destinationTotalAmount: (json['destination_total_amount'] as int?) ?? 0,
        multisigSigCount: (json['multisig_sig_count'] as int?) ?? 0,
      );
}

class SignedTxSetKeyImageEntry {
  final String publicKey;
  final String keyImage;

  const SignedTxSetKeyImageEntry({
    required this.publicKey,
    required this.keyImage,
  });

  factory SignedTxSetKeyImageEntry.fromJson(Map<String, dynamic> json) =>
      SignedTxSetKeyImageEntry(
        publicKey: json['public_key'] as String,
        keyImage: json['key_image'] as String,
      );
}

class SignedTxSetExtractedResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final List<ExtractedSignedTransaction> transactions;
  final List<String> keyImages;
  final List<SignedTxSetKeyImageEntry> txKeyImages;

  const SignedTxSetExtractedResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    required this.transactions,
    this.keyImages = const [],
    this.txKeyImages = const [],
  });

  factory SignedTxSetExtractedResponse.fromJson(Map<String, dynamic> json) =>
      SignedTxSetExtractedResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        transactions: (json['transactions'] as List)
            .map(
              (e) => ExtractedSignedTransaction.fromJson(
                e as Map<String, dynamic>,
              ),
            )
            .toList(),
        keyImages: ((json['key_images'] as List?) ?? const []).cast<String>(),
        txKeyImages: ((json['tx_key_images'] as List?) ?? const [])
            .map(
              (e) =>
                  SignedTxSetKeyImageEntry.fromJson(e as Map<String, dynamic>),
            )
            .toList(),
      );

  static Stream<SignedTxSetExtractedResponse> get stream => signalSender
      .onRawSignal('SignedTxSetExtractedResponse')
      .map(SignedTxSetExtractedResponse.fromJson);
}

class UnsignedTxSetSourceRingEntry {
  final int globalOutputIndex;
  final String outputPublicKey;
  final String commitment;

  const UnsignedTxSetSourceRingEntry({
    required this.globalOutputIndex,
    required this.outputPublicKey,
    required this.commitment,
  });

  factory UnsignedTxSetSourceRingEntry.fromJson(Map<String, dynamic> json) =>
      UnsignedTxSetSourceRingEntry(
        globalOutputIndex: json['global_output_index'] as int,
        outputPublicKey: json['output_public_key'] as String,
        commitment: json['commitment'] as String,
      );
}

class UnsignedTxSetSourceSummary {
  final int ringSize;
  final List<UnsignedTxSetSourceRingEntry> ring;
  final int realOutput;
  final int realGlobalOutputIndex;
  final String realOutputPublicKey;
  final String realTxPublicKey;
  final List<String> realOutAdditionalTxKeys;
  final int realOutputInTxIndex;
  final int amount;
  final bool rct;
  final String mask;

  const UnsignedTxSetSourceSummary({
    required this.ringSize,
    this.ring = const [],
    required this.realOutput,
    required this.realGlobalOutputIndex,
    required this.realOutputPublicKey,
    required this.realTxPublicKey,
    this.realOutAdditionalTxKeys = const [],
    required this.realOutputInTxIndex,
    required this.amount,
    required this.rct,
    this.mask = '',
  });

  factory UnsignedTxSetSourceSummary.fromJson(Map<String, dynamic> json) =>
      UnsignedTxSetSourceSummary(
        ringSize: json['ring_size'] as int,
        ring: ((json['ring'] as List?) ?? const [])
            .map(
              (e) => UnsignedTxSetSourceRingEntry.fromJson(
                e as Map<String, dynamic>,
              ),
            )
            .toList(),
        realOutput: json['real_output'] as int,
        realGlobalOutputIndex: json['real_global_output_index'] as int,
        realOutputPublicKey: json['real_output_public_key'] as String,
        realTxPublicKey: json['real_tx_public_key'] as String,
        realOutAdditionalTxKeys:
            ((json['real_out_additional_tx_keys'] as List?) ?? const [])
                .cast<String>(),
        realOutputInTxIndex: json['real_output_in_tx_index'] as int,
        amount: json['amount'] as int,
        rct: json['rct'] as bool,
        mask: (json['mask'] as String?) ?? '',
      );
}

class UnsignedTxSetDestinationSummary {
  final String originalAddressHex;
  final int amount;
  final String spendPublicKey;
  final String viewPublicKey;
  final bool isSubaddress;
  final bool isIntegrated;

  const UnsignedTxSetDestinationSummary({
    required this.originalAddressHex,
    required this.amount,
    required this.spendPublicKey,
    required this.viewPublicKey,
    required this.isSubaddress,
    required this.isIntegrated,
  });

  static const empty = UnsignedTxSetDestinationSummary(
    originalAddressHex: '',
    amount: 0,
    spendPublicKey: '',
    viewPublicKey: '',
    isSubaddress: false,
    isIntegrated: false,
  );

  factory UnsignedTxSetDestinationSummary.fromJson(Map<String, dynamic> json) =>
      UnsignedTxSetDestinationSummary(
        originalAddressHex: json['original_address_hex'] as String,
        amount: json['amount'] as int,
        spendPublicKey: json['spend_public_key'] as String,
        viewPublicKey: json['view_public_key'] as String,
        isSubaddress: json['is_subaddress'] as bool,
        isIntegrated: json['is_integrated'] as bool,
      );
}

class UnsignedTxSetConstructionSummary {
  final int sourceCount;
  final List<int> sourceRingSizes;
  final List<UnsignedTxSetSourceSummary> sources;
  final int changeAmount;
  final UnsignedTxSetDestinationSummary change;
  final int splitDestinationCount;
  final int splitDestinationTotalAmount;
  final List<UnsignedTxSetDestinationSummary> splitDestinations;
  final List<int> selectedTransferIndices;
  final String extraHex;
  final int unlockTime;
  final int constructionFlags;
  final bool useRct;
  final bool useViewTags;
  final int rctRangeProofType;
  final int rctBpVersion;
  final int destinationCount;
  final int destinationTotalAmount;
  final List<UnsignedTxSetDestinationSummary> destinations;
  final int subaddrAccount;
  final List<int> subaddrIndices;

  const UnsignedTxSetConstructionSummary({
    required this.sourceCount,
    required this.sourceRingSizes,
    required this.sources,
    required this.changeAmount,
    required this.change,
    required this.splitDestinationCount,
    required this.splitDestinationTotalAmount,
    required this.splitDestinations,
    required this.selectedTransferIndices,
    required this.extraHex,
    required this.unlockTime,
    this.constructionFlags = 0,
    required this.useRct,
    required this.useViewTags,
    required this.rctRangeProofType,
    required this.rctBpVersion,
    required this.destinationCount,
    required this.destinationTotalAmount,
    required this.destinations,
    required this.subaddrAccount,
    required this.subaddrIndices,
  });

  factory UnsignedTxSetConstructionSummary.fromJson(
    Map<String, dynamic> json,
  ) => UnsignedTxSetConstructionSummary(
    sourceCount: json['source_count'] as int,
    sourceRingSizes: (json['source_ring_sizes'] as List).cast<int>(),
    sources: (json['sources'] as List)
        .map(
          (e) => UnsignedTxSetSourceSummary.fromJson(e as Map<String, dynamic>),
        )
        .toList(),
    changeAmount: json['change_amount'] as int,
    change: json['change'] != null
        ? UnsignedTxSetDestinationSummary.fromJson(
            json['change'] as Map<String, dynamic>,
          )
        : UnsignedTxSetDestinationSummary.empty,
    splitDestinationCount: json['split_destination_count'] as int,
    splitDestinationTotalAmount: json['split_destination_total_amount'] as int,
    splitDestinations: ((json['split_destinations'] as List?) ?? const [])
        .map(
          (e) => UnsignedTxSetDestinationSummary.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    selectedTransferIndices: (json['selected_transfer_indices'] as List)
        .cast<int>(),
    extraHex: json['extra_hex'] as String,
    unlockTime: json['unlock_time'] as int,
    constructionFlags: (json['construction_flags'] as int?) ?? 0,
    useRct: json['use_rct'] as bool,
    useViewTags: json['use_view_tags'] as bool,
    rctRangeProofType: json['rct_range_proof_type'] as int,
    rctBpVersion: json['rct_bp_version'] as int,
    destinationCount: json['destination_count'] as int,
    destinationTotalAmount: json['destination_total_amount'] as int,
    destinations: (json['destinations'] as List)
        .map(
          (e) => UnsignedTxSetDestinationSummary.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
    subaddrAccount: json['subaddr_account'] as int,
    subaddrIndices: (json['subaddr_indices'] as List).cast<int>(),
  );
}

class UnsignedTxSetTransferSummary {
  final String outputPublicKey;
  final int internalOutputIndex;
  final int globalOutputIndex;
  final String txPublicKey;
  final int flagsRaw;
  final bool spent;
  final bool frozen;
  final bool rct;
  final bool keyImageKnown;
  final bool keyImageRequest;
  final bool keyImagePartial;
  final int amount;
  final List<String> additionalTxKeys;
  final int subaddressMajor;
  final int subaddressMinor;

  const UnsignedTxSetTransferSummary({
    required this.outputPublicKey,
    required this.internalOutputIndex,
    required this.globalOutputIndex,
    required this.txPublicKey,
    required this.flagsRaw,
    required this.spent,
    required this.frozen,
    required this.rct,
    required this.keyImageKnown,
    required this.keyImageRequest,
    required this.keyImagePartial,
    required this.amount,
    required this.additionalTxKeys,
    required this.subaddressMajor,
    required this.subaddressMinor,
  });

  factory UnsignedTxSetTransferSummary.fromJson(Map<String, dynamic> json) =>
      UnsignedTxSetTransferSummary(
        outputPublicKey: json['output_public_key'] as String,
        internalOutputIndex: json['internal_output_index'] as int,
        globalOutputIndex: json['global_output_index'] as int,
        txPublicKey: json['tx_public_key'] as String,
        flagsRaw: json['flags_raw'] as int,
        spent: json['spent'] as bool,
        frozen: json['frozen'] as bool,
        rct: json['rct'] as bool,
        keyImageKnown: json['key_image_known'] as bool,
        keyImageRequest: json['key_image_request'] as bool,
        keyImagePartial: json['key_image_partial'] as bool,
        amount: json['amount'] as int,
        additionalTxKeys: ((json['additional_tx_keys'] as List?) ?? const [])
            .cast<String>(),
        subaddressMajor: json['subaddress_major'] as int,
        subaddressMinor: json['subaddress_minor'] as int,
      );
}

class UnsignedTxSetInspectedResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final int archiveVersion;
  final int transactionCount;
  final int newTransferFirst;
  final int newTransferSecond;
  final int newTransferCount;
  final List<UnsignedTxSetTransferSummary> newTransfers;
  final List<UnsignedTxSetConstructionSummary> constructions;

  const UnsignedTxSetInspectedResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    required this.archiveVersion,
    required this.transactionCount,
    this.newTransferFirst = 0,
    this.newTransferSecond = 0,
    required this.newTransferCount,
    this.newTransfers = const [],
    required this.constructions,
  });

  factory UnsignedTxSetInspectedResponse.fromJson(
    Map<String, dynamic> json,
  ) => UnsignedTxSetInspectedResponse(
    success: json['success'] as bool,
    error: json['error'] as String?,
    errorCode: json['error_code'] as int?,
    errorHint: json['error_hint'] as String?,
    errorTransient: json['error_transient'] as bool?,
    archiveVersion: json['archive_version'] as int,
    transactionCount: json['transaction_count'] as int,
    newTransferFirst: (json['new_transfer_first'] as int?) ?? 0,
    newTransferSecond: (json['new_transfer_second'] as int?) ?? 0,
    newTransferCount: json['new_transfer_count'] as int,
    newTransfers: ((json['new_transfers'] as List?) ?? const [])
        .map(
          (e) =>
              UnsignedTxSetTransferSummary.fromJson(e as Map<String, dynamic>),
        )
        .toList(),
    constructions: (json['constructions'] as List)
        .map(
          (e) => UnsignedTxSetConstructionSummary.fromJson(
            e as Map<String, dynamic>,
          ),
        )
        .toList(),
  );

  static Stream<UnsignedTxSetInspectedResponse> get stream => signalSender
      .onRawSignal('UnsignedTxSetInspectedResponse')
      .map(UnsignedTxSetInspectedResponse.fromJson);
}

class KeyImagesExportedResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final String? keyImagesHex;
  final int count;

  const KeyImagesExportedResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    this.keyImagesHex,
    required this.count,
  });

  factory KeyImagesExportedResponse.fromJson(Map<String, dynamic> json) =>
      KeyImagesExportedResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        keyImagesHex: json['key_images_hex'] as String?,
        count: json['count'] as int,
      );

  static Stream<KeyImagesExportedResponse> get stream => signalSender
      .onRawSignal('KeyImagesExportedResponse')
      .map(KeyImagesExportedResponse.fromJson);
}

class KeyImagesImportedResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final int importedCount;
  final int spentCount;
  final List<String> keyImages;
  final List<String> spentKeyImages;

  const KeyImagesImportedResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    required this.importedCount,
    required this.spentCount,
    this.keyImages = const [],
    this.spentKeyImages = const [],
  });

  factory KeyImagesImportedResponse.fromJson(Map<String, dynamic> json) =>
      KeyImagesImportedResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        importedCount: json['imported_count'] as int,
        spentCount: json['spent_count'] as int,
        keyImages: ((json['key_images'] as List?) ?? const []).cast<String>(),
        spentKeyImages: ((json['spent_key_images'] as List?) ?? const [])
            .cast<String>(),
      );

  static Stream<KeyImagesImportedResponse> get stream => signalSender
      .onRawSignal('KeyImagesImportedResponse')
      .map(KeyImagesImportedResponse.fromJson);
}

class ImportKeysFileResponse {
  final bool success;
  final String? error;
  final int? errorCode;
  final String? errorHint;
  final bool? errorTransient;
  final String? spendSecretKey;
  final String? viewSecretKey;
  final String? spendPublicKey;
  final String? viewPublicKey;
  final int creationTimestamp;
  final bool watchOnly;
  final String? network;
  final String? seedLanguage;
  final String? mnemonic;

  const ImportKeysFileResponse({
    required this.success,
    this.error,
    this.errorCode,
    this.errorHint,
    this.errorTransient,
    this.spendSecretKey,
    this.viewSecretKey,
    this.spendPublicKey,
    this.viewPublicKey,
    required this.creationTimestamp,
    required this.watchOnly,
    this.network,
    this.seedLanguage,
    this.mnemonic,
  });

  factory ImportKeysFileResponse.fromJson(Map<String, dynamic> json) =>
      ImportKeysFileResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        errorCode: json['error_code'] as int?,
        errorHint: json['error_hint'] as String?,
        errorTransient: json['error_transient'] as bool?,
        spendSecretKey: json['spend_secret_key'] as String?,
        viewSecretKey: json['view_secret_key'] as String?,
        spendPublicKey: json['spend_public_key'] as String?,
        viewPublicKey: json['view_public_key'] as String?,
        creationTimestamp: json['creation_timestamp'] as int,
        watchOnly: json['watch_only'] as bool,
        network: json['network'] as String?,
        seedLanguage: json['seed_language'] as String?,
        mnemonic: json['mnemonic'] as String?,
      );

  static Stream<ImportKeysFileResponse> get stream => signalSender
      .onRawSignal('ImportKeysFileResponse')
      .map(ImportKeysFileResponse.fromJson);
}

class ExportKeysFileResponse {
  final bool success;
  final String? error;
  final String? fileBytesHex;

  const ExportKeysFileResponse({
    required this.success,
    this.error,
    this.fileBytesHex,
  });

  factory ExportKeysFileResponse.fromJson(Map<String, dynamic> json) =>
      ExportKeysFileResponse(
        success: json['success'] as bool,
        error: json['error'] as String?,
        fileBytesHex: json['file_bytes_hex'] as String?,
      );

  static Stream<ExportKeysFileResponse> get stream => signalSender
      .onRawSignal('ExportKeysFileResponse')
      .map(ExportKeysFileResponse.fromJson);
}

class QrFrameResponse {
  final List<bool> modules;
  final int size;
  final int seqNum;
  final int seqLen;
  final String uri;

  const QrFrameResponse({
    required this.modules,
    required this.size,
    required this.seqNum,
    required this.seqLen,
    required this.uri,
  });

  factory QrFrameResponse.fromJson(Map<String, dynamic> json) =>
      QrFrameResponse(
        modules: (json['modules'] as List).cast<bool>(),
        size: json['size'] as int,
        seqNum: json['seq_num'] as int,
        seqLen: json['seq_len'] as int,
        uri: json['uri'] as String,
      );

  static Stream<QrFrameResponse> get stream =>
      signalSender.onRawSignal('QrFrameResponse').map(QrFrameResponse.fromJson);
}

class UrDecodeProgressResponse {
  final double progress;
  final bool isComplete;

  const UrDecodeProgressResponse({
    required this.progress,
    required this.isComplete,
  });

  factory UrDecodeProgressResponse.fromJson(Map<String, dynamic> json) =>
      UrDecodeProgressResponse(
        progress: (json['progress'] as num).toDouble(),
        isComplete: json['is_complete'] as bool,
      );

  static Stream<UrDecodeProgressResponse> get stream => signalSender
      .onRawSignal('UrDecodeProgressResponse')
      .map(UrDecodeProgressResponse.fromJson);
}

class UrDecodeCompleteResponse {
  final String dataHex;
  final String urType;

  const UrDecodeCompleteResponse({required this.dataHex, required this.urType});

  factory UrDecodeCompleteResponse.fromJson(Map<String, dynamic> json) =>
      UrDecodeCompleteResponse(
        dataHex: json['data_hex'] as String,
        urType: json['ur_type'] as String,
      );

  static Stream<UrDecodeCompleteResponse> get stream => signalSender
      .onRawSignal('UrDecodeCompleteResponse')
      .map(UrDecodeCompleteResponse.fromJson);
}
