import 'dart:async';
import '../src/ffi/signal_types.dart';
import 'crypto_backend.dart';

class RustCryptoBackend implements CryptoBackend {
  @override
  Future<String?> encrypt(String password, String plaintext) async {
    final completer = Completer<String?>();
    final subscription =
        WalletDataSavedResponse.stream.listen((response) {
      if (!completer.isCompleted) {
        if (response.success && response.encryptedData != null) {
          completer.complete(response.encryptedData);
        } else {
          completer.complete(null);
        }
      }
    });

    SaveWalletDataRequest(
      password: password,
      walletDataJson: plaintext,
    ).sendSignalToRust();

    final result = await completer.future.timeout(
      const Duration(seconds: 10),
      onTimeout: () => null,
    );

    await subscription.cancel();
    return result;
  }

  @override
  Future<({String keyHex, String saltHex})?> deriveKey(String password) async {
    final completer = Completer<({String keyHex, String saltHex})?>();
    final subscription =
        EncryptionKeyDerivedResponse.stream.listen((response) {
      if (!completer.isCompleted) {
        if (response.success &&
            response.keyHex != null &&
            response.saltHex != null) {
          completer.complete((
            keyHex: response.keyHex!,
            saltHex: response.saltHex!,
          ));
        } else {
          completer.complete(null);
        }
      }
    });

    DeriveEncryptionKeyRequest(
      password: password,
    ).sendSignalToRust();

    final result = await completer.future.timeout(
      const Duration(seconds: 10),
      onTimeout: () => null,
    );

    await subscription.cancel();
    return result;
  }

  @override
  Future<String?> encryptWithKey(
      String keyHex, String saltHex, String plaintext) async {
    final completer = Completer<String?>();
    final subscription =
        WalletDataSavedResponse.stream.listen((response) {
      if (!completer.isCompleted) {
        if (response.success && response.encryptedData != null) {
          completer.complete(response.encryptedData);
        } else {
          completer.complete(null);
        }
      }
    });

    SaveWithDerivedKeyRequest(
      keyHex: keyHex,
      saltHex: saltHex,
      walletDataJson: plaintext,
    ).sendSignalToRust();

    final result = await completer.future.timeout(
      const Duration(seconds: 10),
      onTimeout: () => null,
    );

    await subscription.cancel();
    return result;
  }

  @override
  Future<String?> decrypt(String password, String ciphertext) async {
    final completer = Completer<String?>();
    final subscription =
        WalletDataLoadedResponse.stream.listen((response) {
      if (!completer.isCompleted) {
        if (response.success && response.walletDataJson != null) {
          completer.complete(response.walletDataJson);
        } else {
          completer.complete(null);
        }
      }
    });

    LoadWalletDataRequest(
      password: password,
      encryptedData: ciphertext,
    ).sendSignalToRust();

    final result = await completer.future.timeout(
      const Duration(seconds: 10),
      onTimeout: () => null,
    );

    await subscription.cancel();
    return result;
  }
}
