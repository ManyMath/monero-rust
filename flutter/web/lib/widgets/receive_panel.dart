import 'package:flutter/material.dart';
import '../src/ffi/signal_types.dart';

/// Widget that displays receive addresses interface.
///
/// Shows account expansion panels with scan controls and unused subaddresses.
/// Scan checkboxes control which accounts are sent to the Rust continuous
/// scanner for account-filtered block scanning.
class ReceivePanel extends StatefulWidget {
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
  final Function(int accountIndex, bool shouldScan)
  onScanToggle; // Toggle scan for account

  const ReceivePanel({
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
  State<ReceivePanel> createState() => _ReceivePanelState();
}

class _ReceivePanelState extends State<ReceivePanel> {
  bool _showUsedSubaddresses = false;
  int? _expandedIndex;

  @override
  void initState() {
    super.initState();
    // Expand first account by default
    if (widget.accounts.isNotEmpty) {
      _expandedIndex = 0;
      // Trigger initial subaddress derivation for Account 0
      WidgetsBinding.instance.addPostFrameCallback((_) {
        widget.onAccountSelected(widget.accounts[0]);
      });
    }
  }

  @override
  void didUpdateWidget(ReceivePanel oldWidget) {
    super.didUpdateWidget(oldWidget);

    // Only update if accounts have changed
    if (oldWidget.accounts != widget.accounts) {
      setState(() {
        // Check if a new account was added (list grew)
        if (widget.accounts.length > oldWidget.accounts.length &&
            widget.accounts.isNotEmpty) {
          // Keep the current expanded index stable - don't auto-switch
          // when scanner discovers new accounts via lookahead
        }
        // Clean up expanded index if it's out of bounds
        else if (_expandedIndex != null &&
            _expandedIndex! >= widget.accounts.length) {
          _expandedIndex = null;
        }
        // If no account is expanded but we have accounts, expand the first one
        else if (_expandedIndex == null && widget.accounts.isNotEmpty) {
          _expandedIndex = 0;
        }
      });
    }
  }

  /// Group all outputs by account in a single pass.
  /// Returns a map from account number to (subaddress index -> first output info).
  Map<int, Map<int, _SubaddressInfo>> _groupOutputsByAccount() {
    final grouped = <int, Map<int, _SubaddressInfo>>{};
    for (var output in widget.allOutputs) {
      if (output.subaddressIndex != null) {
        final subIdx = output.subaddressIndex!;
        final outputAccount = subIdx.$1;
        final addressIndex = subIdx.$2;
        final accountMap = grouped.putIfAbsent(
          outputAccount,
          () => <int, _SubaddressInfo>{},
        );
        // Store the first output found for this subaddress
        if (!accountMap.containsKey(addressIndex)) {
          accountMap[addressIndex] = _SubaddressInfo(
            txHash: output.txHash,
            blockHeight: output.blockHeight,
          );
        }
      }
    }
    return grouped;
  }

  /// Get the first 5 unused subaddress indices for an account,
  /// given the pre-computed set of used indices.
  List<int> _getUnusedSubaddresses(Map<int, _SubaddressInfo> usedSubaddresses) {
    final usedKeys = usedSubaddresses.keys.toSet();
    final unused = <int>[];

    int index = 0;
    while (unused.length < 5) {
      if (!usedKeys.contains(index)) {
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
        child: Text('Enter a seed phrase to receive funds'),
      );
    }

    // Pre-compute outputs grouped by account in a single pass
    final outputsByAccount = _groupOutputsByAccount();

    // If only one account, show its contents directly
    if (widget.accounts.length == 1) {
      final account = widget.accounts[0];
      final usedSubaddresses =
          outputsByAccount[account] ?? <int, _SubaddressInfo>{};
      final unusedIndices = _getUnusedSubaddresses(usedSubaddresses);
      final isScanning = widget.scanningAccounts.contains(account);

      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          // Scan checkbox for single account
          Padding(
            padding: const EdgeInsets.all(16.0),
            child: Row(
              children: [
                Text(
                  'Account $account',
                  style: const TextStyle(fontWeight: FontWeight.w500),
                ),
                const Spacer(),
                const Text('Scan'),
                Checkbox(
                  value: isScanning,
                  onChanged: (value) {
                    widget.onScanToggle(account, value ?? false);
                  },
                ),
              ],
            ),
          ),
          const Divider(height: 1),
          // Account contents
          _buildAccountBody(account, unusedIndices, usedSubaddresses),
          const Divider(height: 1),
          // Create new account button
          Padding(
            padding: const EdgeInsets.all(16.0),
            child: Center(
              child: ElevatedButton.icon(
                onPressed: widget.onCreateAccount,
                icon: const Icon(Icons.add, size: 16),
                label: const Text('Create new account'),
                style: ElevatedButton.styleFrom(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 12,
                    vertical: 8,
                  ),
                ),
              ),
            ),
          ),
        ],
      );
    }

    // Multiple accounts - use expansion tiles
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        // Account expansion tiles
        Padding(
          padding: const EdgeInsets.all(16.0),
          child: Card(
            elevation: 1,
            color: Colors.white,
            child: Column(
              children: widget.accounts.asMap().entries.map<Widget>((entry) {
                final index = entry.key;
                final account = entry.value;
                final isExpanded = _expandedIndex == index;
                final usedSubaddresses =
                    outputsByAccount[account] ?? <int, _SubaddressInfo>{};
                final unusedIndices = _getUnusedSubaddresses(usedSubaddresses);
                final isScanning = widget.scanningAccounts.contains(account);

                return Theme(
                  data: Theme.of(
                    context,
                  ).copyWith(splashColor: Theme.of(context).hoverColor),
                  child: ExpansionTile(
                    key: Key(
                      'account-$account-$isExpanded',
                    ), // Force rebuild when expansion state changes
                    initiallyExpanded: isExpanded,
                    onExpansionChanged: (expanded) {
                      setState(() {
                        _expandedIndex = expanded ? index : null;
                      });
                      if (expanded) {
                        widget.onAccountSelected(account);
                      }
                    },
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
                    children: [
                      _buildAccountBody(
                        account,
                        unusedIndices,
                        usedSubaddresses,
                      ),
                    ],
                  ),
                );
              }).toList(),
            ),
          ),
        ),

        const Divider(height: 1),

        // Create new account button at the bottom
        Padding(
          padding: const EdgeInsets.all(16.0),
          child: Center(
            child: ElevatedButton.icon(
              onPressed: widget.onCreateAccount,
              icon: const Icon(Icons.add, size: 16),
              label: const Text('Create new account'),
              style: ElevatedButton.styleFrom(
                padding: const EdgeInsets.symmetric(
                  horizontal: 12,
                  vertical: 8,
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildAccountBody(
    int account,
    List<int> unusedIndices,
    Map<int, _SubaddressInfo> usedSubaddresses,
  ) {
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
              style: TextStyle(
                fontWeight: FontWeight.w400,
                fontSize: 12,
                color: Colors.grey,
              ),
            ),
            const SizedBox(height: 8),
            for (var entry in usedSubaddresses.entries)
              if (widget.subaddresses['$account,${entry.key}'] != null &&
                  widget.subaddresses['$account,${entry.key}']!.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(bottom: 8.0),
                  child: InkWell(
                    onTap: widget.onNavigateToTransaction != null
                        ? () => widget.onNavigateToTransaction!(
                            entry.value.txHash,
                          )
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
                                  style: const TextStyle(
                                    fontSize: 11,
                                    fontWeight: FontWeight.w500,
                                  ),
                                ),
                                Text(
                                  'Height: ${entry.value.blockHeight}',
                                  style: TextStyle(
                                    fontSize: 10,
                                    color: Colors.blue.shade700,
                                  ),
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
            if (widget.subaddresses['$account,$addressIndex'] == null ||
                widget.subaddresses['$account,$addressIndex']!.isEmpty)
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

  _SubaddressInfo({required this.txHash, required this.blockHeight});
}
