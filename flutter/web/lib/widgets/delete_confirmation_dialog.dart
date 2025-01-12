import 'package:flutter/material.dart';

class DeleteConfirmationDialog {
  /// Shows a confirmation dialog for deleting stored wallet data.
  /// Returns true if user confirms deletion, false/null otherwise.
  static Future<bool?> show(BuildContext context, String walletId) {
    return showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Clear Stored Data'),
        content: Text(
          'Are you sure you want to clear stored data for wallet "$walletId"? This cannot be undone.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Cancel'),
          ),
          ElevatedButton(
            onPressed: () => Navigator.of(context).pop(true),
            style: ElevatedButton.styleFrom(
              backgroundColor: Colors.red,
              foregroundColor: Colors.white,
            ),
            child: const Text('Clear'),
          ),
        ],
      ),
    );
  }
}
