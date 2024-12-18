import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

class ClipboardUtils {
  /// Copy text to clipboard and show a snackbar notification
  static Future<void> copyToClipboard(
    BuildContext context,
    String text,
    String label,
  ) async {
    await Clipboard.setData(ClipboardData(text: text));
    if (context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('$label copied to clipboard'),
          duration: const Duration(seconds: 2),
        ),
      );
    }
  }
}
