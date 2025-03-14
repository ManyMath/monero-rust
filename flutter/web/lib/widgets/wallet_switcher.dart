import 'package:flutter/material.dart';

class WalletSwitcher extends StatelessWidget {
  final List<String> availableWalletIds;
  final String walletId;
  final String? lastSaveTime;
  final bool isLoadingWallet;
  final bool isSaving;
  final String? seed;
  final int transactionCount;
  final int outputCount;
  final ValueChanged<String> onWalletChanged;
  final VoidCallback onNew;
  final VoidCallback onSave;
  final VoidCallback onLoad;
  final VoidCallback onDelete;

  const WalletSwitcher({
    super.key,
    required this.availableWalletIds,
    required this.walletId,
    required this.lastSaveTime,
    required this.isLoadingWallet,
    required this.isSaving,
    required this.seed,
    required this.transactionCount,
    required this.outputCount,
    required this.onWalletChanged,
    required this.onNew,
    required this.onSave,
    required this.onLoad,
    required this.onDelete,
  });

  bool get hasWalletData =>
      (seed?.trim().isNotEmpty ?? false) ||
      transactionCount > 0 ||
      outputCount > 0;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: Colors.grey.shade100,
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: Colors.grey.shade300),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.account_balance_wallet, color: Colors.grey.shade700, size: 20),
              const SizedBox(width: 8),
              Text(
                'Stored Wallets',
                style: TextStyle(
                  fontWeight: FontWeight.bold,
                  color: Colors.grey.shade900,
                ),
              ),
            ],
          ),
          const SizedBox(height: 12),
          DropdownButtonFormField<String>(
            value: availableWalletIds.contains(walletId) ? walletId : null,
            decoration: const InputDecoration(
              labelText: 'Select Wallet',
              border: OutlineInputBorder(),
              contentPadding: EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            ),
            items: availableWalletIds.map((id) {
              return DropdownMenuItem(
                value: id,
                child: Text(id),
              );
            }).toList(),
            onChanged: (newId) {
              if (newId != null) {
                onWalletChanged(newId);
              }
            },
          ),
          if (lastSaveTime != null) ...[
            const SizedBox(height: 8),
            Text(
              'Last saved: $lastSaveTime',
              style: TextStyle(
                fontSize: 12,
                color: Colors.grey.shade700,
              ),
            ),
          ],
          const SizedBox(height: 12),
          Row(
            children: [
              Expanded(
                child: OutlinedButton.icon(
                  onPressed: onNew,
                  icon: const Icon(Icons.add),
                  label: const Text('New'),
                ),
              ),
              if (hasWalletData) ...[
                const SizedBox(width: 8),
                Expanded(
                  child: ElevatedButton.icon(
                    onPressed: isSaving ? null : onSave,
                    icon: isSaving
                        ? const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Icon(Icons.save),
                    label: Text(isSaving ? 'Saving...' : 'Save'),
                  ),
                ),
              ],
              const SizedBox(width: 8),
              Expanded(
                child: ElevatedButton.icon(
                  onPressed: isLoadingWallet ? null : onLoad,
                  icon: isLoadingWallet
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.folder_open),
                  label: Text(isLoadingWallet ? 'Loading...' : 'Load'),
                  style: ElevatedButton.styleFrom(
                    backgroundColor: Colors.green,
                    foregroundColor: Colors.white,
                  ),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: OutlinedButton.icon(
                  onPressed: onDelete,
                  icon: const Icon(Icons.delete_outline),
                  label: const Text('Delete'),
                  style: OutlinedButton.styleFrom(
                    foregroundColor: Colors.red,
                  ),
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}
