/// Utility functions for network-related operations.
class NetworkUtils {
  /// Normalize node URL by adding http:// prefix if missing.
  ///
  /// Trims whitespace and ensures the URL has a scheme (http:// or https://).
  /// If no scheme is present, defaults to http://.
  static String normalizeNodeUrl(String url) {
    final trimmed = url.trim();
    if (trimmed.startsWith('http://') || trimmed.startsWith('https://')) {
      return trimmed;
    }
    return 'http://$trimmed';
  }
}
