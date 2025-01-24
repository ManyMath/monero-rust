import 'dart:async';
import '../src/bindings/bindings.dart';
import 'crypto_backend.dart';

class RustCryptoBackend implements CryptoBackend {
  @override
  Future<String?> encrypt(String password, String plaintext) async {
    final completer = Completer<String?>();
    final subscription =
        WalletDataSavedResponse.rustSignalStream.listen((signal) {
      if (!completer.isCompleted) {
        if (signal.message.success && signal.message.encryptedData != null) {
          completer.complete(signal.message.encryptedData);
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
  Future<String?> decrypt(String password, String ciphertext) async {
    final completer = Completer<String?>();
    final subscription =
        WalletDataLoadedResponse.rustSignalStream.listen((signal) {
      if (!completer.isCompleted) {
        if (signal.message.success && signal.message.walletDataJson != null) {
          completer.complete(signal.message.walletDataJson);
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
