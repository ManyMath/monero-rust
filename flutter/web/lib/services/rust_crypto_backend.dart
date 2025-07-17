import 'dart:async';
import '../src/bindings/bindings.dart';
import 'crypto_backend.dart';

class RustCryptoBackend implements CryptoBackend {
  @override
  Future<String?> encrypt(String password, String plaintext) async {
    final completer = Completer<String?>();
    final subscription =
        WalletDataSavedResponse.stream.listen((msg) {
      if (!completer.isCompleted) {
        if (msg.success && msg.encryptedData != null) {
          completer.complete(msg.encryptedData);
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
        EncryptionKeyDerivedResponse.stream.listen((msg) {
      if (!completer.isCompleted) {
        if (msg.success &&
            msg.keyHex != null &&
            msg.saltHex != null) {
          completer.complete((
            keyHex: msg.keyHex!,
            saltHex: msg.saltHex!,
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
        WalletDataSavedResponse.stream.listen((msg) {
      if (!completer.isCompleted) {
        if (msg.success && msg.encryptedData != null) {
          completer.complete(msg.encryptedData);
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
        WalletDataLoadedResponse.stream.listen((msg) {
      if (!completer.isCompleted) {
        if (msg.success && msg.walletDataJson != null) {
          completer.complete(msg.walletDataJson);
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
