import 'package:flutter/material.dart';
import '../models/wallet_instance.dart';

class LoadedWalletsDisplay extends StatelessWidget {
  final List<WalletInstance> activeWallets;
  final String? activeWalletId;
  final Function(String) onCloseWallet;
  final Function(String) onWalletChanged;

  const LoadedWalletsDisplay({
    super.key,
    required this.activeWallets,
    required this.activeWalletId,
    required this.onCloseWallet,
    required this.onWalletChanged,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(12),
      decoration: BoxDecoration(
        color: Colors.green.shade50,
        borderRadius: BorderRadius.circular(8),
        border: Border.all(color: Colors.green.shade200),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Icon(Icons.wallet, color: Colors.green.shade700, size: 20),
              const SizedBox(width: 8),
              Text(
                'Loaded Wallets (${activeWallets.length})',
                style: TextStyle(
                  fontWeight: FontWeight.bold,
                  color: Colors.grey.shade900,
                ),
              ),
            ],
          ),
          const SizedBox(height: 12),
          if (activeWallets.length > 1) ...[
            Row(
              children: [
                SizedBox(
                  width: 48,
                  child: Center(
                    child: Text(
                      'Active',
                      style: TextStyle(
                        fontSize: 12,
                        fontWeight: FontWeight.bold,
                        color: Colors.grey.shade700,
                      ),
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    'Wallet',
                    style: TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.bold,
                      color: Colors.grey.shade700,
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                const SizedBox(width: 18),
              ],
            ),
            const SizedBox(height: 8),
          ],
          ...activeWallets.map((wallet) {
            return Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: Row(
                children: [
                  if (activeWallets.length > 1) ...[
                    SizedBox(
                      width: 48,
                      child: Center(
                        child: Radio<String>(
                          value: wallet.walletId,
                          groupValue: activeWalletId,
                          onChanged: (value) {
                            if (value != null) {
                              onWalletChanged(value);
                            }
                          },
                          visualDensity: VisualDensity.compact,
                        ),
                      ),
                    ),
                    const SizedBox(width: 8),
                  ],
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          wallet.walletId,
                          style: TextStyle(
                            fontWeight: wallet.walletId == activeWalletId
                                ? FontWeight.bold
                                : FontWeight.normal,
                          ),
                        ),
                        Text(
                          '${wallet.address.substring(0, 20)}... | ${wallet.totalBalance.toStringAsFixed(6)} XMR',
                          style: TextStyle(
                            fontSize: 11,
                            color: Colors.grey.shade600,
                          ),
                        ),
                        if (wallet.currentHeight > 0)
                          Text(
                            'Block: ${wallet.currentHeight}',
                            style: TextStyle(
                              fontSize: 10,
                              color: Colors.grey.shade500,
                            ),
                          ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 8),
                  IconButton(
                    icon: const Icon(Icons.close, size: 18),
                    color: Colors.red.shade400,
                    tooltip: 'Close/unload wallet',
                    onPressed: () => onCloseWallet(wallet.walletId),
                    padding: EdgeInsets.zero,
                    constraints: const BoxConstraints(),
                  ),
                ],
              ),
            );
          }),
        ],
      ),
    );
  }
}
