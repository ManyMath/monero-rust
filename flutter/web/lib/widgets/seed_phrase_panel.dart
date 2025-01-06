import 'package:flutter/material.dart';
import '../utils/key_parser.dart';
import '../src/bindings/bindings.dart';
import '../utils/clipboard_utils.dart';

class SeedPhrasePanel extends StatelessWidget {
  final TextEditingController controller;
  final String seedType;
  final String network;
  final String? validationError;
  final String? responseError;
  final VoidCallback onGenerateSeed;
  final ValueChanged<String> onNetworkChanged;

  const SeedPhrasePanel({
    super.key,
    required this.controller,
    required this.seedType,
    required this.network,
    required this.validationError,
    required this.responseError,
    required this.onGenerateSeed,
    required this.onNetworkChanged,
  });

  Future<void> _copyToClipboard(BuildContext context, String text, String label) async {
    await ClipboardUtils.copyToClipboard(context, text, label);
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16.0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              ElevatedButton.icon(
                onPressed: onGenerateSeed,
                icon: const Icon(Icons.auto_awesome),
                label: const Text('Generate'),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: DropdownButtonFormField<String>(
                  value: seedType,
                  decoration: const InputDecoration(
                    labelText: 'Seed Type',
                    border: OutlineInputBorder(),
                    contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                  ),
                  items: const [
                    DropdownMenuItem(value: '25 word', child: Text('25 word')),
                  ],
                  onChanged: null,
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: DropdownButtonFormField<String>(
                  value: network,
                  decoration: const InputDecoration(
                    labelText: 'Network',
                    border: OutlineInputBorder(),
                    contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                  ),
                  items: const [
                    DropdownMenuItem(value: 'mainnet', child: Text('Mainnet')),
                    DropdownMenuItem(value: 'testnet', child: Text('Testnet')),
                    DropdownMenuItem(value: 'stagenet', child: Text('Stagenet')),
                  ],
                  onChanged: (value) {
                    if (value != null) {
                      onNetworkChanged(value);
                    }
                  },
                ),
              ),
            ],
          ),
          const SizedBox(height: 16),
          Row(
            children: [
              Expanded(
                child: TextField(
                  controller: controller,
                  decoration: InputDecoration(
                    labelText: 'Seed Phrase',
                    hintText: 'Enter or generate a 25-word seed phrase',
                    border: const OutlineInputBorder(),
                    errorText: validationError,
                  ),
                  maxLines: 3,
                ),
              ),
              IconButton(
                icon: const Icon(Icons.copy_outlined),
                onPressed: () => _copyToClipboard(context, controller.text, 'Seed'),
                tooltip: 'Copy seed',
              ),
            ],
          ),
          if (responseError != null) ...[
            const SizedBox(height: 16),
            Container(
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: Colors.red.shade50,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: Colors.red.shade200),
              ),
              child: SelectableText(
                'Error: $responseError',
                style: TextStyle(color: Colors.red.shade900),
              ),
            ),
          ],
        ],
      ),
    );
  }
}
