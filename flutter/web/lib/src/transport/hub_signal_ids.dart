/// Numeric signal IDs shared between Rust and Dart.
///
/// IMPORTANT: Must stay in sync with `signal_ids.rs` on the Rust side.
class HubSignalIds {
  HubSignalIds._();

  // -- DartSignal IDs (Dart -> Rust) --
  static const int moneroTestRequest = 1;
  static const int createWalletRequest = 2;
  static const int startSyncRequest = 3;
  static const int getBalanceRequest = 4;
  static const int createTransactionRequest = 5;
  static const int sweepAllRequest = 6;
  static const int generateSeedRequest = 7;
  static const int getSeedBirthdayRequest = 8;
  static const int getBlockHeightFromTimestampRequest = 9;
  static const int deriveAddressRequest = 10;
  static const int deriveSubaddressRequest = 11;
  static const int deriveKeysRequest = 12;
  static const int scanBlockRequest = 13;
  static const int broadcastTransactionRequest = 14;
  static const int queryDaemonHeightRequest = 15;
  static const int startContinuousScanRequest = 16;
  static const int stopScanRequest = 17;
  static const int mempoolScanRequest = 18;
  static const int generateOutProofRequest = 19;
  static const int saveWalletDataRequest = 20;
  static const int loadWalletDataRequest = 21;
  static const int deriveEncryptionKeyRequest = 22;
  static const int saveWithDerivedKeyRequest = 23;
  static const int scanBlockMultiWalletRequest = 24;
  static const int startMultiWalletScanRequest = 25;
  static const int restoreWalletDataRequest = 26;
  static const int getBlockHashesRequest = 27;
  static const int getPendingStateRequest = 28;
  static const int convertBip39ToLegacyRequest = 29;
  static const int freezeOutputRequest = 30;
  static const int thawOutputRequest = 31;
  static const int createUnsignedTransactionRequest = 32;
  static const int signUnsignedTransactionRequest = 33;
  static const int exportKeyImagesRequest = 34;
  static const int importKeyImagesRequest = 35;
  static const int importKeysFileRequest = 36;
  static const int exportKeysFileRequest = 37;
  static const int startUrEncoderRequest = 38;
  static const int stopUrEncoderRequest = 39;
  static const int urDecodeFrameRequest = 40;
  static const int resetUrDecoderRequest = 41;

  // -- RustSignal IDs (Rust -> Dart) --
  static const int moneroTestResponse = 101;
  static const int walletCreatedResponse = 102;
  static const int syncProgressResponse = 103;
  static const int balanceResponse = 104;
  static const int transactionCreatedResponse = 105;
  static const int seedGeneratedResponse = 106;
  static const int seedBirthdayResponse = 107;
  static const int blockHeightFromTimestampResponse = 108;
  static const int addressDerivedResponse = 109;
  static const int subaddressDerivedResponse = 110;
  static const int keysDerivedResponse = 111;
  static const int blockScanResponse = 112;
  static const int transactionBroadcastResponse = 113;
  static const int daemonHeightResponse = 114;
  static const int spentStatusUpdatedResponse = 115;
  static const int mempoolScanResponse = 116;
  static const int outProofGeneratedResponse = 117;
  static const int walletDataSavedResponse = 118;
  static const int walletDataLoadedResponse = 119;
  static const int encryptionKeyDerivedResponse = 120;
  static const int multiWalletScanResponse = 121;
  static const int blockHashesResponse = 122;
  static const int pendingStateResponse = 123;
  static const int reorgDetectedResponse = 124;
  static const int doubleSpendDetectedResponse = 125;
  static const int bip39LegacySeedResponse = 126;
  static const int freezeThawResponse = 127;
  static const int transactionStatusUpdate = 128;
  static const int unsignedTransactionCreatedResponse = 129;
  static const int transactionSignedOfflineResponse = 130;
  static const int keyImagesExportedResponse = 131;
  static const int keyImagesImportedResponse = 132;
  static const int importKeysFileResponse = 133;
  static const int exportKeysFileResponse = 134;
  static const int qrFrameResponse = 135;
  static const int urDecodeProgressResponse = 136;
  static const int urDecodeCompleteResponse = 137;
}
