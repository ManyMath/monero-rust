abstract class CryptoBackend {
  Future<String?> encrypt(String password, String plaintext);
  Future<String?> decrypt(String password, String ciphertext);
  Future<({String keyHex, String saltHex})?> deriveKey(String password);
  Future<String?> encryptWithKey(String keyHex, String saltHex, String plaintext);
}
