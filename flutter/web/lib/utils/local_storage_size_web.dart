import 'dart:html' as html;

int calculateMoneroWalletLocalStorageBytes() {
  var totalBytes = 0;
  for (var i = 0; i < html.window.localStorage.length; i++) {
    final key = html.window.localStorage.keys.elementAt(i);
    if (key.startsWith('monero_wallet_')) {
      final value = html.window.localStorage[key];
      if (value != null) {
        totalBytes += value.length;
      }
    }
  }
  return totalBytes;
}
