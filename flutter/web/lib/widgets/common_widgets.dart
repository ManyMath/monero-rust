import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

/// Common reusable widget helpers for displaying various UI elements.
/// These widgets were extracted from debug_view.dart to promote code reuse
/// across the application.
class CommonWidgets {
  /// Builds a key-value row with a copy button.
  ///
  /// Displays a label and value in a row with a copy button. The copy button
  /// is disabled if the value is 'TODO'.
  ///
  /// - [label]: The label text to display
  /// - [value]: The value to display (can be 'TODO')
  /// - [onCopyPressed]: Callback when the copy button is pressed
  static Widget buildKeyRow({
    required String label,
    required String value,
    required VoidCallback onCopyPressed,
  }) {
    final bool isTodo = value == 'TODO';
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  label,
                  style: const TextStyle(fontWeight: FontWeight.w500, fontSize: 13),
                ),
                const SizedBox(height: 4),
                SelectableText(
                  value,
                  style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
                ),
              ],
            ),
          ),
          IconButton(
            icon: const Icon(Icons.copy_outlined, size: 16),
            onPressed: !isTodo ? onCopyPressed : null,
            tooltip: isTodo ? null : 'Copy $label',
            padding: EdgeInsets.zero,
            constraints: const BoxConstraints(),
          ),
        ],
      ),
    );
  }

  /// Builds a scan result row displaying label and value.
  ///
  /// Used for displaying scan results in a consistent format.
  ///
  /// - [label]: The label text to display
  /// - [value]: The value to display
  static Widget buildScanResultRow({
    required String label,
    required String value,
  }) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 120,
            child: Text(
              '$label:',
              style: const TextStyle(fontWeight: FontWeight.w500, fontSize: 12),
            ),
          ),
          Expanded(
            child: SelectableText(
              value,
              style: const TextStyle(fontSize: 12, fontFamily: 'monospace'),
            ),
          ),
        ],
      ),
    );
  }

  /// Builds an output detail row with optional monospace font.
  ///
  /// Used for displaying output field details in a consistent format.
  ///
  /// - [label]: The label text to display
  /// - [value]: The value to display
  /// - [mono]: Whether to use monospace font for the value (default: false)
  static Widget buildOutputDetailRow({
    required String label,
    required String value,
    bool mono = false,
  }) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 100,
            child: Text(
              '$label:',
              style: const TextStyle(
                fontWeight: FontWeight.w500,
                fontSize: 11,
                color: Colors.black54,
              ),
            ),
          ),
          Expanded(
            child: SelectableText(
              value,
              style: TextStyle(
                fontSize: 11,
                fontFamily: mono ? 'monospace' : null,
              ),
            ),
          ),
        ],
      ),
    );
  }

  /// Builds a sortable column header button.
  ///
  /// Displays a button that shows the current sort state (active/inactive)
  /// and direction (ascending/descending) with arrow indicators.
  ///
  /// - [label]: The label text to display
  /// - [sortKey]: The sort key identifier
  /// - [currentSortKey]: The currently active sort key
  /// - [isAscending]: Whether the current sort is ascending
  /// - [onTap]: Callback when the button is tapped
  static Widget buildSortButton({
    required String label,
    required String sortKey,
    required String currentSortKey,
    required bool isAscending,
    required VoidCallback onTap,
  }) {
    final isActive = currentSortKey == sortKey;
    final arrow = isActive ? (isAscending ? ' ↑' : ' ↓') : '';
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(4),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
        decoration: BoxDecoration(
          color: isActive ? Colors.blue.shade100 : Colors.grey.shade200,
          borderRadius: BorderRadius.circular(4),
        ),
        child: Text(
          '$label$arrow',
          style: TextStyle(
            fontSize: 12,
            fontWeight: isActive ? FontWeight.bold : FontWeight.normal,
          ),
        ),
      ),
    );
  }

  /// Builds a transaction sort button.
  ///
  /// Similar to buildSortButton but specifically for transaction sorting.
  /// Displays a button that shows the current sort state (active/inactive)
  /// and direction (ascending/descending) with arrow indicators.
  ///
  /// - [label]: The label text to display
  /// - [sortKey]: The sort key identifier
  /// - [currentSortKey]: The currently active sort key
  /// - [isAscending]: Whether the current sort is ascending
  /// - [onTap]: Callback when the button is tapped
  static Widget buildTxSortButton({
    required String label,
    required String sortKey,
    required String currentSortKey,
    required bool isAscending,
    required VoidCallback onTap,
  }) {
    final isActive = currentSortKey == sortKey;
    final arrow = isActive ? (isAscending ? ' ↑' : ' ↓') : '';
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(4),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
        decoration: BoxDecoration(
          color: isActive ? Colors.blue.shade100 : Colors.grey.shade200,
          borderRadius: BorderRadius.circular(4),
        ),
        child: Text(
          '$label$arrow',
          style: TextStyle(
            fontSize: 12,
            fontWeight: isActive ? FontWeight.bold : FontWeight.normal,
          ),
        ),
      ),
    );
  }

  /// Builds a selection button for selecting/deselecting items.
  ///
  /// Used for actions like "Select All" or "Clear Selection".
  ///
  /// - [label]: The label text to display
  /// - [onPressed]: Callback when the button is pressed
  static Widget buildSelectButton({
    required String label,
    required VoidCallback onPressed,
  }) {
    return InkWell(
      onTap: onPressed,
      borderRadius: BorderRadius.circular(4),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
        decoration: BoxDecoration(
          color: Colors.grey.shade200,
          borderRadius: BorderRadius.circular(4),
        ),
        child: Text(
          label,
          style: const TextStyle(fontSize: 12),
        ),
      ),
    );
  }

  /// Builds a proof detail row with a copy button.
  ///
  /// Displays a label and value with a copy button that shows a snackbar
  /// when clicked.
  ///
  /// - [label]: The label text to display
  /// - [value]: The value to display
  /// - [context]: BuildContext for showing snackbar
  static Widget buildProofRow({
    required String label,
    required String value,
    required BuildContext context,
  }) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SizedBox(
          width: 70,
          child: Text(label, style: const TextStyle(fontWeight: FontWeight.bold, fontSize: 12)),
        ),
        Expanded(
          child: SelectableText(value, style: const TextStyle(fontFamily: 'monospace', fontSize: 11)),
        ),
        IconButton(
          icon: const Icon(Icons.copy, size: 16),
          padding: EdgeInsets.zero,
          constraints: const BoxConstraints(),
          tooltip: 'Copy',
          onPressed: () {
            Clipboard.setData(ClipboardData(text: value));
            ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('$label copied')));
          },
        ),
      ],
    );
  }
}
