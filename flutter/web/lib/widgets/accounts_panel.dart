import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';

/// Widget that displays account management interface.
///
/// Shows account selection dropdown, create account button, and unused subaddresses.
class AccountsPanel extends StatefulWidget {
  final String? seed;
  final String network;
  final int activeAccount;
  final List<int> accounts;
  final List<OwnedOutput> allOutputs;
  final Function(int accountIndex) onAccountSelected;
  final VoidCallback onCreateAccount;
  final Function(String text, String label) onCopyToClipboard;
  final Map<String, String> subaddresses; // (account, index) -> address
  final Function(String txHash)? onNavigateToTransaction;

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
    this.onNavigateToTransaction,
  });

  @override
  State<AccountsPanel> createState() => _AccountsPanelState();
}

class _AccountsPanelState extends State<AccountsPanel> {
  bool _showUsedSubaddresses = false;

  /// Get used subaddress indices for an account with transaction info
  Map<int, _SubaddressInfo> _getUsedSubaddressesWithTxInfo(int account) {
    final used = <int, _SubaddressInfo>{};
    for (var output in widget.allOutputs) {
      if (output.subaddressIndex != null) {
        final subIdx = output.subaddressIndex!;
        final outputAccount = subIdx.item1;
        final addressIndex = subIdx.item2;
        if (outputAccount == account) {
          // Store the first output found for this subaddress
          if (!used.containsKey(addressIndex)) {
            used[addressIndex] = _SubaddressInfo(
              txHash: output.txHash,
              blockHeight: output.blockHeight.toInt(),
            );
          }
        }
      }
    }
    return used;
  }

  /// Get the first 5 unused subaddress indices for an account
  List<int> _getUnusedSubaddresses(int account) {
    final used = _getUsedSubaddressesWithTxInfo(account).keys.toSet();
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

  /// Get the first 3 unused subaddress indices for each account (for "All" view)
  Map<int, List<int>> _getUnusedSubaddressesPerAccount() {
    final result = <int, List<int>>{};

    for (var account in widget.accounts) {
      final used = _getUsedSubaddressesWithTxInfo(account).keys.toSet();
      final unused = <int>[];

      int index = 0;
      while (unused.length < 3) {
        if (!used.contains(index)) {
          unused.add(index);
        }
        index++;
      }

      result[account] = unused;
    }

    return result;
  }

  @override
  Widget build(BuildContext context) {
    if (widget.seed == null || widget.seed!.isEmpty) {
      return const Padding(
        padding: EdgeInsets.all(16.0),
        child: Text('Enter a seed phrase to manage accounts'),
      );
    }

    // Get subaddresses based on whether specific account or "All" is selected
    final unusedIndices = widget.activeAccount >= 0 ? _getUnusedSubaddresses(widget.activeAccount) : <int>[];
    final unusedPerAccount = widget.activeAccount == -1 ? _getUnusedSubaddressesPerAccount() : <int, List<int>>{};
    final usedSubaddresses = widget.activeAccount >= 0 ? _getUsedSubaddressesWithTxInfo(widget.activeAccount) : <int, _SubaddressInfo>{};

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
                  value: widget.activeAccount,
                  isExpanded: true,
                  items: [
                    // Add "All" option if there are multiple accounts
                    if (widget.accounts.length > 1)
                      const DropdownMenuItem(
                        value: -1,
                        child: Text('All'),
                      ),
                    // Add individual account options
                    ...widget.accounts
                        .map((account) => DropdownMenuItem(
                              value: account,
                              child: Text(account.toString()),
                            ))
                        .toList(),
                  ],
                  onChanged: widget.accounts.length <= 1
                      ? null // Disable if only one account
                      : (value) {
                          if (value != null) {
                            widget.onAccountSelected(value);
                          }
                        },
                ),
              ),
              const SizedBox(width: 16),
              ElevatedButton.icon(
                onPressed: widget.onCreateAccount,
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

        // Subaddresses section (only show for specific accounts, not "All")
        if (widget.activeAccount >= 0)
          Padding(
            padding: const EdgeInsets.all(16.0),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                // Toggle for showing used subaddresses
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    const Text(
                      'Subaddresses:',
                      style: TextStyle(fontWeight: FontWeight.w500, fontSize: 13),
                    ),
                    if (usedSubaddresses.isNotEmpty)
                      Row(
                        children: [
                          Text(
                            'Show used (${usedSubaddresses.length})',
                            style: const TextStyle(fontSize: 12),
                          ),
                          Switch(
                            value: _showUsedSubaddresses,
                            onChanged: (value) {
                              setState(() {
                                _showUsedSubaddresses = value;
                              });
                            },
                            materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
                          ),
                        ],
                      ),
                  ],
                ),
                const SizedBox(height: 8),

                // Show used subaddresses if toggled on
                if (_showUsedSubaddresses && usedSubaddresses.isNotEmpty) ...[
                  const Text(
                    'Used Subaddresses:',
                    style: TextStyle(fontWeight: FontWeight.w400, fontSize: 12, color: Colors.grey),
                  ),
                  const SizedBox(height: 8),
                  ...usedSubaddresses.entries.map((entry) {
                    final addressIndex = entry.key;
                    final info = entry.value;
                    final key = '${widget.activeAccount},$addressIndex';
                    final address = widget.subaddresses[key];

                    if (address == null || address.isEmpty) {
                      return const SizedBox.shrink();
                    }

                    return Padding(
                      padding: const EdgeInsets.only(bottom: 8.0),
                      child: InkWell(
                        onTap: widget.onNavigateToTransaction != null
                            ? () => widget.onNavigateToTransaction!(info.txHash)
                            : null,
                        borderRadius: BorderRadius.circular(4),
                        child: Container(
                          padding: const EdgeInsets.all(8),
                          decoration: BoxDecoration(
                            color: Colors.grey.shade50,
                            borderRadius: BorderRadius.circular(4),
                            border: Border.all(color: Colors.grey.shade300),
                          ),
                          child: Row(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              SizedBox(
                                width: 60,
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Text(
                                      'Index $addressIndex',
                                      style: const TextStyle(fontSize: 11, fontWeight: FontWeight.w500),
                                    ),
                                    Text(
                                      'Height: ${info.blockHeight}',
                                      style: TextStyle(fontSize: 10, color: Colors.blue.shade700),
                                    ),
                                  ],
                                ),
                              ),
                              Expanded(
                                child: SelectableText(
                                  address,
                                  style: const TextStyle(
                                    fontFamily: 'monospace',
                                    fontSize: 10,
                                  ),
                                ),
                              ),
                              IconButton(
                                icon: const Icon(Icons.copy_outlined, size: 14),
                                onPressed: () => widget.onCopyToClipboard(
                                  address,
                                  'Used Subaddress ${widget.activeAccount}/$addressIndex',
                                ),
                                tooltip: 'Copy subaddress',
                                padding: EdgeInsets.zero,
                                constraints: const BoxConstraints(),
                              ),
                            ],
                          ),
                        ),
                      ),
                    );
                  }).toList(),
                  const SizedBox(height: 12),
                ],

                const Text(
                  'Unused Subaddresses (first 5):',
                  style: TextStyle(fontWeight: FontWeight.w400, fontSize: 12),
                ),
                const SizedBox(height: 8),
                ...unusedIndices.map((addressIndex) {
                final key = '${widget.activeAccount},$addressIndex';
                final address = widget.subaddresses[key];

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
                        onPressed: () => widget.onCopyToClipboard(
                          address,
                          'Subaddress ${widget.activeAccount}/$addressIndex',
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
          )
        else if (widget.activeAccount == -1)
          // "All" accounts view - show first 3 unused subaddresses per account
          Padding(
            padding: const EdgeInsets.all(16.0),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Text(
                  'Unused Subaddresses (first 3 per account):',
                  style: TextStyle(fontWeight: FontWeight.w500, fontSize: 13),
                ),
                const SizedBox(height: 8),
                ...unusedPerAccount.entries.map((entry) {
                  final account = entry.key;
                  final indices = entry.value;

                  return Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Padding(
                        padding: const EdgeInsets.only(top: 8.0, bottom: 4.0),
                        child: Text(
                          'Account $account:',
                          style: const TextStyle(
                            fontWeight: FontWeight.w600,
                            fontSize: 12,
                            color: Colors.black87,
                          ),
                        ),
                      ),
                      ...indices.map((addressIndex) {
                        final key = '$account,$addressIndex';
                        final address = widget.subaddresses[key];

                        if (address == null || address.isEmpty) {
                          return Padding(
                            padding: const EdgeInsets.only(bottom: 6.0, left: 8.0),
                            child: Row(
                              children: [
                                SizedBox(
                                  width: 50,
                                  child: Text(
                                    'Idx $addressIndex:',
                                    style: const TextStyle(fontSize: 11),
                                  ),
                                ),
                                const Expanded(
                                  child: Text(
                                    'Loading...',
                                    style: TextStyle(
                                      fontFamily: 'monospace',
                                      fontSize: 10,
                                      color: Colors.grey,
                                    ),
                                  ),
                                ),
                              ],
                            ),
                          );
                        }

                        return Padding(
                          padding: const EdgeInsets.only(bottom: 6.0, left: 8.0),
                          child: Row(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              SizedBox(
                                width: 50,
                                child: Text(
                                  'Idx $addressIndex:',
                                  style: const TextStyle(fontSize: 11),
                                ),
                              ),
                              Expanded(
                                child: SelectableText(
                                  address,
                                  style: const TextStyle(
                                    fontFamily: 'monospace',
                                    fontSize: 10,
                                  ),
                                ),
                              ),
                              IconButton(
                                icon: const Icon(Icons.copy_outlined, size: 12),
                                onPressed: () => widget.onCopyToClipboard(
                                  address,
                                  'Subaddress $account/$addressIndex',
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
                  );
                }).toList(),
              ],
            ),
          ),
      ],
    );
  }
}

/// Helper class to store subaddress transaction information
class _SubaddressInfo {
  final String txHash;
  final int blockHeight;

  _SubaddressInfo({
    required this.txHash,
    required this.blockHeight,
  });
}
