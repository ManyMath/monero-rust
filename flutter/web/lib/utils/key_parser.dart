class KeyParseResult {
  final bool isValid;
  final String? normalizedInput;
  final String? error;

  KeyParseResult.valid(this.normalizedInput)
      : isValid = true,
        error = null;

  KeyParseResult.invalid(this.error)
      : isValid = false,
        normalizedInput = null;
}

class KeyParser {
  static KeyParseResult parse(String input) {
    if (input.trim().isEmpty) {
      return KeyParseResult.invalid('Input is empty');
    }

    // View-only sentinel passes through as-is
    if (input.trim().startsWith('viewonly:')) {
      return KeyParseResult.valid(input.trim());
    }

    final normalized = input.trim().replaceAll(RegExp(r'\s+'), ' ');
    final words = normalized.split(' ');

    // Accept 12-word (BIP39), 16-word (polyseed), and 25-word (classic) seeds
    if (words.length != 12 && words.length != 16 && words.length != 25) {
      return KeyParseResult.invalid('Expected 12, 16, or 25 words, got ${words.length}');
    }

    for (final word in words) {
      if (word.isEmpty) {
        return KeyParseResult.invalid('Invalid word format');
      }
    }

    return KeyParseResult.valid(normalized);
  }

}
