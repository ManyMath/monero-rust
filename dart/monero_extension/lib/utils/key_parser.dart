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

    final normalized = input.trim().replaceAll(RegExp(r'\s+'), ' ');
    final words = normalized.split(' ');

    if (words.length != 25) {
      return KeyParseResult.invalid('Expected 25 words, got ${words.length}');
    }

    for (final word in words) {
      if (word.isEmpty) {
        return KeyParseResult.invalid('Invalid word format');
      }
    }

    return KeyParseResult.valid(normalized);
  }
}
