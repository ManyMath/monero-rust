import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';

/// Widget that displays account management interface.
///
/// Shows account selection dropdown, create account button, and unused subaddresses.
class AccountsPanel extends StatelessWidget {
  final String? seed;
  final String network;
  final int activeAccount;
  final List<int> accounts;
  final List<OwnedOutput> allOutputs;
  final Function(int accountIndex) onAccountSelected;
  final VoidCallback onCreateAccount;
  final Function(String text, String label) onCopyToClipboard;
  final Map<String, String> subaddresses; // (account, index) -> address

  const AccountsPanel({
    super.key,
    required this.seed,
    required this.network,
    required this.activeAccount,
    required this.accounts,
    required this.allOutputs,
    required this.onAccountSelected,
    required this.onCreateAccount,
    required this.onCopyToClipboard,
    required this.subaddresses,
  });

  /// Get used subaddress indices for an account
  Set<int> _getUsedSubaddresses(int account) {
    final used = <int>{};
    for (var output in allOutputs) {
      if (output.subaddressIndex != null) {
        final subIdx = output.subaddressIndex!;
        final outputAccount = subIdx.item1;
        final addressIndex = subIdx.item2;
        if (outputAccount == account) {
          used.add(addressIndex);
        }
      }
    }
    return used;
  }

  /// Get the first 5 unused subaddress indices for an account
  List<int> _getUnusedSubaddresses(int account) {
    final used = _getUsedSubaddresses(account);
    final unused = <int>[];

    int index = 0;
    while (unused.length < 5) {
      if (!used.contains(index)) {
        unused.add(index);
      }
      index++;
    }

    return unused;
  }

  @override
  Widget build(BuildContext context) {
    if (seed == null || seed!.isEmpty) {
      return const Padding(
        padding: EdgeInsets.all(16.0),
        child: Text('Enter a seed phrase to manage accounts'),
      );
    }

    final unusedIndices = _getUnusedSubaddresses(activeAccount);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // Account selection section
        Padding(
          padding: const EdgeInsets.all(16.0),
          child: Row(
            children: [
              const Text(
                'Account:',
                style: TextStyle(fontWeight: FontWeight.w500),
              ),
              const SizedBox(width: 8),
              SizedBox(
                width: 100,
                child: DropdownButton<int>(
                  value: activeAccount,
                  isExpanded: true,
                  items: accounts
                      .map((account) => DropdownMenuItem(
                            value: account,
                            child: Text(account.toString()),
                          ))
                      .toList(),
                  onChanged: accounts.length <= 1
                      ? null // Disable if only one account
                      : (value) {
                          if (value != null) {
                            onAccountSelected(value);
                          }
                        },
                ),
              ),
              const SizedBox(width: 16),
              ElevatedButton.icon(
                onPressed: onCreateAccount,
                icon: const Icon(Icons.add, size: 16),
                label: const Text('Create new account'),
                style: ElevatedButton.styleFrom(
                  padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                ),
              ),
            ],
          ),
        ),
        const Divider(height: 1),

        // Subaddresses section
        Padding(
          padding: const EdgeInsets.all(16.0),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const Text(
                'Unused Subaddresses (first 5):',
                style: TextStyle(fontWeight: FontWeight.w500, fontSize: 13),
              ),
              const SizedBox(height: 8),
              ...unusedIndices.map((addressIndex) {
                final key = '$activeAccount,$addressIndex';
                final address = subaddresses[key];

                if (address == null || address.isEmpty) {
                  return Padding(
                    padding: const EdgeInsets.only(bottom: 8.0),
                    child: Row(
                      children: [
                        SizedBox(
                          width: 60,
                          child: Text(
                            'Index $addressIndex:',
                            style: const TextStyle(fontSize: 12),
                          ),
                        ),
                        const Expanded(
                          child: Text(
                            'Loading...',
                            style: TextStyle(
                              fontFamily: 'monospace',
                              fontSize: 11,
                              color: Colors.grey,
                            ),
                          ),
                        ),
                      ],
                    ),
                  );
                }

                return Padding(
                  padding: const EdgeInsets.only(bottom: 8.0),
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      SizedBox(
                        width: 60,
                        child: Text(
                          'Index $addressIndex:',
                          style: const TextStyle(fontSize: 12),
                        ),
                      ),
                      Expanded(
                        child: SelectableText(
                          address,
                          style: const TextStyle(
                            fontFamily: 'monospace',
                            fontSize: 11,
                          ),
                        ),
                      ),
                      IconButton(
                        icon: const Icon(Icons.copy_outlined, size: 14),
                        onPressed: () => onCopyToClipboard(
                          address,
                          'Subaddress $activeAccount/$addressIndex',
                        ),
                        tooltip: 'Copy subaddress',
                        padding: EdgeInsets.zero,
                        constraints: const BoxConstraints(),
                      ),
                    ],
                  ),
                );
              }).toList(),
            ],
          ),
        ),
      ],
    );
  }
}
