import 'package:flutter/material.dart';
import 'common_widgets.dart';

/// Widget that displays wallet keys and address.
///
/// Shows the Monero address and public/private spend/view keys with copy buttons.
class KeysDisplayPanel extends StatelessWidget {
  final String? address;
  final String? secretSpendKey;
  final String? secretViewKey;
  final String? publicSpendKey;
  final String? publicViewKey;
  final String network;
  final Function(String text, String label) onCopyToClipboard;

  const KeysDisplayPanel({
    super.key,
    required this.address,
    required this.secretSpendKey,
    required this.secretViewKey,
    required this.publicSpendKey,
    required this.publicViewKey,
    required this.network,
    required this.onCopyToClipboard,
  });

  @override
  Widget build(BuildContext context) {
    if (address == null) {
      return const Padding(
        padding: EdgeInsets.all(16.0),
        child: Text('Enter a seed phrase to view keys'),
      );
    }

    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                '${network[0].toUpperCase()}${network.substring(1)} Address',
                style: const TextStyle(fontWeight: FontWeight.w500, fontSize: 13),
              ),
              const SizedBox(height: 4),
              Row(
                children: [
                  Expanded(
                    child: SelectableText(
                      address!,
                      style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
                    ),
                  ),
                  IconButton(
                    icon: const Icon(Icons.copy_outlined, size: 16),
                    onPressed: () => onCopyToClipboard(address!, 'Address'),
                    tooltip: 'Copy address',
                    padding: EdgeInsets.zero,
                    constraints: const BoxConstraints(),
                  ),
                ],
              ),
            ],
          ),
        ),
        const Divider(height: 1),
        CommonWidgets.buildKeyRow(
          label: 'Secret Spend Key',
          value: secretSpendKey ?? 'TODO',
          onCopyPressed: () => onCopyToClipboard(secretSpendKey ?? '', 'Secret Spend Key'),
        ),
        CommonWidgets.buildKeyRow(
          label: 'Secret View Key',
          value: secretViewKey ?? 'TODO',
          onCopyPressed: () => onCopyToClipboard(secretViewKey ?? '', 'Secret View Key'),
        ),
        CommonWidgets.buildKeyRow(
          label: 'Public Spend Key',
          value: publicSpendKey ?? 'TODO',
          onCopyPressed: () => onCopyToClipboard(publicSpendKey ?? '', 'Public Spend Key'),
        ),
        CommonWidgets.buildKeyRow(
          label: 'Public View Key',
          value: publicViewKey ?? 'TODO',
          onCopyPressed: () => onCopyToClipboard(publicViewKey ?? '', 'Public View Key'),
        ),
      ],
    );
  }
}
