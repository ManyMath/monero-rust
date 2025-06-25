part of 'signals.dart';

final assignRustSignal = <String, void Function(Uint8List, Uint8List)>{
  'AddressDerivedResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = AddressDerivedResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _addressDerivedResponseStreamController.add(rustSignal);
    AddressDerivedResponse.latestRustSignal = rustSignal;
  },
  'BalanceResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = BalanceResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _balanceResponseStreamController.add(rustSignal);
    BalanceResponse.latestRustSignal = rustSignal;
  },
  'Bip39LegacySeedResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = Bip39LegacySeedResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _bip39LegacySeedResponseStreamController.add(rustSignal);
    Bip39LegacySeedResponse.latestRustSignal = rustSignal;
  },
  'BlockHashesResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = BlockHashesResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _blockHashesResponseStreamController.add(rustSignal);
    BlockHashesResponse.latestRustSignal = rustSignal;
  },
  'BlockHeightFromTimestampResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = BlockHeightFromTimestampResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _blockHeightFromTimestampResponseStreamController.add(rustSignal);
    BlockHeightFromTimestampResponse.latestRustSignal = rustSignal;
  },
  'BlockScanResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = BlockScanResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _blockScanResponseStreamController.add(rustSignal);
    BlockScanResponse.latestRustSignal = rustSignal;
  },
  'DaemonHeightResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = DaemonHeightResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _daemonHeightResponseStreamController.add(rustSignal);
    DaemonHeightResponse.latestRustSignal = rustSignal;
  },
  'DoubleSpendDetectedResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = DoubleSpendDetectedResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _doubleSpendDetectedResponseStreamController.add(rustSignal);
    DoubleSpendDetectedResponse.latestRustSignal = rustSignal;
  },
  'EncryptionKeyDerivedResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = EncryptionKeyDerivedResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _encryptionKeyDerivedResponseStreamController.add(rustSignal);
    EncryptionKeyDerivedResponse.latestRustSignal = rustSignal;
  },
  'FreezeThawResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = FreezeThawResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _freezeThawResponseStreamController.add(rustSignal);
    FreezeThawResponse.latestRustSignal = rustSignal;
  },
  'KeysDerivedResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = KeysDerivedResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _keysDerivedResponseStreamController.add(rustSignal);
    KeysDerivedResponse.latestRustSignal = rustSignal;
  },
  'MempoolScanResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = MempoolScanResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _mempoolScanResponseStreamController.add(rustSignal);
    MempoolScanResponse.latestRustSignal = rustSignal;
  },
  'MoneroTestResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = MoneroTestResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _moneroTestResponseStreamController.add(rustSignal);
    MoneroTestResponse.latestRustSignal = rustSignal;
  },
  'MultiWalletScanResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = MultiWalletScanResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _multiWalletScanResponseStreamController.add(rustSignal);
    MultiWalletScanResponse.latestRustSignal = rustSignal;
  },
  'OutProofGeneratedResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = OutProofGeneratedResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _outProofGeneratedResponseStreamController.add(rustSignal);
    OutProofGeneratedResponse.latestRustSignal = rustSignal;
  },
  'PendingStateResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = PendingStateResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _pendingStateResponseStreamController.add(rustSignal);
    PendingStateResponse.latestRustSignal = rustSignal;
  },
  'ReorgDetectedResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = ReorgDetectedResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _reorgDetectedResponseStreamController.add(rustSignal);
    ReorgDetectedResponse.latestRustSignal = rustSignal;
  },
  'SeedBirthdayResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = SeedBirthdayResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _seedBirthdayResponseStreamController.add(rustSignal);
    SeedBirthdayResponse.latestRustSignal = rustSignal;
  },
  'SeedGeneratedResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = SeedGeneratedResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _seedGeneratedResponseStreamController.add(rustSignal);
    SeedGeneratedResponse.latestRustSignal = rustSignal;
  },
  'SpentStatusUpdatedResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = SpentStatusUpdatedResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _spentStatusUpdatedResponseStreamController.add(rustSignal);
    SpentStatusUpdatedResponse.latestRustSignal = rustSignal;
  },
  'SubaddressDerivedResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = SubaddressDerivedResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _subaddressDerivedResponseStreamController.add(rustSignal);
    SubaddressDerivedResponse.latestRustSignal = rustSignal;
  },
  'SyncProgressResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = SyncProgressResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _syncProgressResponseStreamController.add(rustSignal);
    SyncProgressResponse.latestRustSignal = rustSignal;
  },
  'TransactionBroadcastResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = TransactionBroadcastResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _transactionBroadcastResponseStreamController.add(rustSignal);
    TransactionBroadcastResponse.latestRustSignal = rustSignal;
  },
  'TransactionCreatedResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = TransactionCreatedResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _transactionCreatedResponseStreamController.add(rustSignal);
    TransactionCreatedResponse.latestRustSignal = rustSignal;
  },
  'TransactionStatusUpdate': (Uint8List messageBytes, Uint8List binary) {
    final message = TransactionStatusUpdate.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _transactionStatusUpdateStreamController.add(rustSignal);
    TransactionStatusUpdate.latestRustSignal = rustSignal;
  },
  'WalletCreatedResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = WalletCreatedResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _walletCreatedResponseStreamController.add(rustSignal);
    WalletCreatedResponse.latestRustSignal = rustSignal;
  },
  'WalletDataLoadedResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = WalletDataLoadedResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _walletDataLoadedResponseStreamController.add(rustSignal);
    WalletDataLoadedResponse.latestRustSignal = rustSignal;
  },
  'WalletDataSavedResponse': (Uint8List messageBytes, Uint8List binary) {
    final message = WalletDataSavedResponse.bincodeDeserialize(messageBytes);
    final rustSignal = RustSignalPack(
      message,
      binary,
    );
    _walletDataSavedResponseStreamController.add(rustSignal);
    WalletDataSavedResponse.latestRustSignal = rustSignal;
  },
};
