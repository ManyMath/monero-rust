import 'package:flutter/material.dart';
import '../src/bindings/bindings.dart';

/// Widget that displays account management interface.
///
/// Shows account expansion panels with scan controls and unused subaddresses.
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
  final Set<int> scanningAccounts; // Which accounts are being scanned
  final Function(int accountIndex, bool shouldScan) onScanToggle; // Toggle scan for account

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
    required this.scanningAccounts,
    required this.onScanToggle,
  });

  @override
  State<AccountsPanel> createState() => _AccountsPanelState();
}

class _AccountsPanelState extends State<AccountsPanel> {
  bool _showUsedSubaddresses = false;
  Set<int> _expandedAccounts = {};

  @override
  void initState() {
    super.initState();
    // Expand account 0 by default
    if (widget.accounts.isNotEmpty) {
      _expandedAccounts.add(0);
    }
  }

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


  @override
  Widget build(BuildContext context) {
    if (widget.seed == null || widget.seed!.isEmpty) {
      return const Padding(
        padding: EdgeInsets.all(16.0),
        child: Text('Enter a seed phrase to manage accounts'),
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // Header section with create account button
        Padding(
          padding: const EdgeInsets.all(16.0),
          child: Row(
            children: [
              const Text(
                'Accounts',
                style: TextStyle(fontWeight: FontWeight.w600, fontSize: 16),
              ),
              const Spacer(),
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

        // Account expansion panels
        Padding(
          padding: const EdgeInsets.all(16.0),
          child: ExpansionPanelList(
            elevation: 1,
            expandedHeaderPadding: EdgeInsets.zero,
            expansionCallback: (int index, bool isExpanded) {
              setState(() {
                final account = widget.accounts[index];
                if (isExpanded) {
                  _expandedAccounts.remove(account);
                } else {
                  _expandedAccounts.add(account);
                  widget.onAccountSelected(account);
                }
              });
            },
            children: widget.accounts.map<ExpansionPanel>((int account) {
              final isExpanded = _expandedAccounts.contains(account);
              final unusedIndices = isExpanded ? _getUnusedSubaddresses(account) : <int>[];
              final usedSubaddresses = isExpanded ? _getUsedSubaddressesWithTxInfo(account) : <int, _SubaddressInfo>{};
              final isScanning = widget.scanningAccounts.contains(account);

              return ExpansionPanel(
                headerBuilder: (BuildContext context, bool isExpanded) {
                  return ListTile(
                    title: Text(
                      'Account $account',
                      style: const TextStyle(fontWeight: FontWeight.w500),
                    ),
                    trailing: Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        const Text('Scan'),
                        Checkbox(
                          value: isScanning,
                          onChanged: (value) {
                            widget.onScanToggle(account, value ?? false);
                          },
                        ),
                      ],
                    ),
                  );
                },
                body: _buildAccountBody(account, unusedIndices, usedSubaddresses),
                isExpanded: isExpanded,
              );
            }).toList(),
          ),
        ),

      ],
    );
  }

  Widget _buildAccountBody(int account, List<int> unusedIndices, Map<int, _SubaddressInfo> usedSubaddresses) {
    return Padding(
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
            for (var entry in usedSubaddresses.entries)
              if (widget.subaddresses['$account,${entry.key}'] != null && widget.subaddresses['$account,${entry.key}']!.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(bottom: 8.0),
                  child: InkWell(
                    onTap: widget.onNavigateToTransaction != null
                        ? () => widget.onNavigateToTransaction!(entry.value.txHash)
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
                                  'Index ${entry.key}',
                                  style: const TextStyle(fontSize: 11, fontWeight: FontWeight.w500),
                                ),
                                Text(
                                  'Height: ${entry.value.blockHeight}',
                                  style: TextStyle(fontSize: 10, color: Colors.blue.shade700),
                              ),
                            ],
                          ),
                          ),
                          Expanded(
                            child: SelectableText(
                              widget.subaddresses['$account,${entry.key}']!,
                              style: const TextStyle(
                                fontFamily: 'monospace',
                                fontSize: 10,
                              ),
                            ),
                          ),
                          IconButton(
                            icon: const Icon(Icons.copy_outlined, size: 14),
                            onPressed: () => widget.onCopyToClipboard(
                              widget.subaddresses['$account,${entry.key}']!,
                              'Used Subaddress $account/${entry.key}',
                            ),
                            tooltip: 'Copy subaddress',
                            padding: EdgeInsets.zero,
                            constraints: const BoxConstraints(),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
            const SizedBox(height: 12),
          ],

          const Text(
            'Unused Subaddresses (first 5):',
            style: TextStyle(fontWeight: FontWeight.w400, fontSize: 12),
          ),
          const SizedBox(height: 8),
          for (var addressIndex in unusedIndices) ...[
            if (widget.subaddresses['$account,$addressIndex'] == null || widget.subaddresses['$account,$addressIndex']!.isEmpty)
              Padding(
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
              )
            else
              Padding(
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
                        widget.subaddresses['$account,$addressIndex']!,
                        style: const TextStyle(
                          fontFamily: 'monospace',
                          fontSize: 11,
                        ),
                      ),
                    ),
                    IconButton(
                      icon: const Icon(Icons.copy_outlined, size: 14),
                      onPressed: () => widget.onCopyToClipboard(
                        widget.subaddresses['$account,$addressIndex']!,
                        'Subaddress $account/$addressIndex',
                      ),
                      tooltip: 'Copy subaddress',
                      padding: EdgeInsets.zero,
                      constraints: const BoxConstraints(),
                    ),
                  ],
                ),
              ),
          ],
        ],
      ),
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
