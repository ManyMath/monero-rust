import 'package:flutter/material.dart';
import '../utils/clipboard_utils.dart';
import 'error_message_container.dart';

class SeedPhrasePanel extends StatelessWidget {
  final TextEditingController controller;
  final TextEditingController passphraseController;
  final TextEditingController? viewKeyController;
  final TextEditingController? spendKeyController;
  final String seedType;
  final String network;
  final String? validationError;
  final String? responseError;
  final String? derivedLegacySeed;
  final VoidCallback onGenerateSeed;
  final ValueChanged<String> onNetworkChanged;
  final ValueChanged<String> onSeedTypeChanged;

  const SeedPhrasePanel({
    super.key,
    required this.controller,
    required this.passphraseController,
    this.viewKeyController,
    this.spendKeyController,
    required this.seedType,
    required this.network,
    required this.validationError,
    required this.responseError,
    this.derivedLegacySeed,
    required this.onGenerateSeed,
    required this.onNetworkChanged,
    required this.onSeedTypeChanged,
  });

  Future<void> _copyToClipboard(BuildContext context, String text, String label) async {
    await ClipboardUtils.copyToClipboard(context, text, label);
  }

  bool get _isViewOnly => seedType == 'view-only';

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16.0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              if (!_isViewOnly)
                ElevatedButton.icon(
                  onPressed: onGenerateSeed,
                  icon: const Icon(Icons.auto_awesome),
                  label: const Text('Generate'),
                ),
              if (!_isViewOnly) const SizedBox(width: 8),
              Expanded(
                child: DropdownButtonFormField<String>(
                  value: seedType,
                  decoration: const InputDecoration(
                    labelText: 'Seed Type',
                    border: OutlineInputBorder(),
                    contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                  ),
                  items: const [
                    DropdownMenuItem(value: '25 word (classic)', child: Text('25 word (classic)')),
                    DropdownMenuItem(value: '16-word (polyseed)', child: Text('16 word (polyseed)')),
                    DropdownMenuItem(value: '12-word (bip39)', child: Text('12 word (BIP39)')),
                    DropdownMenuItem(value: 'view-only', child: Text('View only')),
                  ],
                  onChanged: (value) {
                    if (value != null) {
                      onSeedTypeChanged(value);
                    }
                  },
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
          if (_isViewOnly) ...[
            TextField(
              controller: viewKeyController,
              decoration: InputDecoration(
                labelText: 'Private View Key (hex)',
                hintText: '64-character hex string',
                border: const OutlineInputBorder(),
                errorText: validationError,
              ),
              maxLines: 1,
            ),
            const SizedBox(height: 8),
            TextField(
              controller: spendKeyController,
              decoration: const InputDecoration(
                labelText: 'Public Spend Key (hex)',
                hintText: '64-character hex string',
                border: OutlineInputBorder(),
              ),
              maxLines: 1,
            ),
          ] else ...[
            Row(
              children: [
                Expanded(
                  child: TextField(
                    controller: controller,
                    decoration: InputDecoration(
                      labelText: 'Seed Phrase',
                      hintText: 'Enter or generate a 12, 16, or 25-word seed phrase',
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
            if (seedType.contains('polyseed') || seedType.contains('bip39')) ...[
              const SizedBox(height: 8),
              TextField(
                controller: passphraseController,
                decoration: InputDecoration(
                  labelText: 'Passphrase (optional)',
                  hintText: seedType.contains('polyseed')
                      ? 'Polyseed passphrase for key derivation'
                      : 'BIP39 passphrase',
                  border: const OutlineInputBorder(),
                ),
                obscureText: true,
              ),
            ],
          ],
          if (derivedLegacySeed != null && !_isViewOnly) ...[
            const SizedBox(height: 8),
            Container(
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: Colors.grey.shade100,
                border: Border.all(color: Colors.grey.shade300),
                borderRadius: BorderRadius.circular(4),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(children: [
                    const Text('Derived 25-word Monero seed:',
                        style: TextStyle(fontWeight: FontWeight.bold, fontSize: 12)),
                    const Spacer(),
                    IconButton(
                      icon: const Icon(Icons.copy_outlined, size: 16),
                      onPressed: () => _copyToClipboard(context, derivedLegacySeed!, 'Legacy Seed'),
                    ),
                  ]),
                  const SizedBox(height: 4),
                  SelectableText(derivedLegacySeed!,
                      style: const TextStyle(fontSize: 12, fontFamily: 'monospace')),
                ],
              ),
            ),
          ],
          if (responseError != null) ...[
            const SizedBox(height: 16),
            ErrorMessageContainer(message: 'Error: $responseError'),
          ],
        ],
      ),
    );
  }
}
