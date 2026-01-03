const signedMoneroTxSetMagicHex = '4d6f6e65726f207369676e65642074782073657405';
const unsignedMoneroTxSetMagicHex =
    '4d6f6e65726f20756e7369676e65642074782073657405';

bool isSignedMoneroTxSetHex(String hex) {
  return hex.trim().toLowerCase().startsWith(signedMoneroTxSetMagicHex);
}

bool isUnsignedMoneroTxSetHex(String hex) {
  return hex.trim().toLowerCase().startsWith(unsignedMoneroTxSetMagicHex);
}

String? viewKeyHexFromViewOnlySeed(String? seed) {
  final trimmed = seed?.trim();
  if (trimmed == null || !trimmed.startsWith('viewonly:')) return null;
  final parts = trimmed.substring('viewonly:'.length).split(':');
  if (parts.length != 2 || parts[0].isEmpty) return null;
  return parts[0];
}
