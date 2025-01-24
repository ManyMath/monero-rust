abstract class CryptoBackend {
  Future<String?> encrypt(String password, String plaintext);
  Future<String?> decrypt(String password, String ciphertext);
}
