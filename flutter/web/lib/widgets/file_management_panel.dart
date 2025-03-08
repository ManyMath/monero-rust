import 'package:flutter/material.dart';
import '../models/wallet_instance.dart';
import 'wallet_switcher.dart';
import 'loaded_wallets_display.dart';
import 'error_message_container.dart';

class FileManagementPanel extends StatelessWidget {
  final String walletId;
  final List<String> availableWalletIds;
  final List<WalletInstance> activeWallets;
  final String? activeWalletId;
  final String? lastSaveTime;
  final bool isSaving;
  final bool isLoadingWallet;
  final bool isExporting;
  final bool isImporting;
  final String? saveError;
  final String? loadError;
  final String? exportError;
  final String? importError;
  final Function(String) onWalletChanged;
  final VoidCallback onLoad;
  final VoidCallback onDelete;
  final VoidCallback onSave;
  final VoidCallback onNew;
  final VoidCallback onExport;
  final VoidCallback onImport;
  final Function(String) onCloseWallet;

  const FileManagementPanel({
    super.key,
    required this.walletId,
    required this.availableWalletIds,
    required this.activeWallets,
    required this.activeWalletId,
    required this.lastSaveTime,
    required this.isSaving,
    required this.isLoadingWallet,
    required this.isExporting,
    required this.isImporting,
    required this.saveError,
    required this.loadError,
    required this.exportError,
    required this.importError,
    required this.onWalletChanged,
    required this.onLoad,
    required this.onDelete,
    required this.onSave,
    required this.onNew,
    required this.onExport,
    required this.onImport,
    required this.onCloseWallet,
  });

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16.0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (availableWalletIds.isNotEmpty) ...[
            WalletSwitcher(
              availableWalletIds: availableWalletIds,
              walletId: walletId,
              lastSaveTime: lastSaveTime,
              isLoadingWallet: isLoadingWallet,
              isSaving: isSaving,
              onWalletChanged: onWalletChanged,
              onNew: onNew,
              onSave: onSave,
              onLoad: onLoad,
              onDelete: onDelete,
            ),
            const SizedBox(height: 12),
          ],

          if (activeWallets.isNotEmpty) ...[
            LoadedWalletsDisplay(
              activeWallets: activeWallets,
              activeWalletId: activeWalletId,
              onCloseWallet: onCloseWallet,
              onWalletChanged: onWalletChanged,
            ),
            const SizedBox(height: 12),
          ],

          Row(
            children: [
              Expanded(
                child: OutlinedButton.icon(
                  onPressed: isExporting ? null : onExport,
                  icon: isExporting
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.file_download),
                  label: Text(isExporting ? 'Exporting...' : 'Export'),
                  style: OutlinedButton.styleFrom(
                    foregroundColor: Colors.blue,
                  ),
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: OutlinedButton.icon(
                  onPressed: isImporting ? null : onImport,
                  icon: isImporting
                      ? const SizedBox(
                          width: 16,
                          height: 16,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.file_upload),
                  label: Text(isImporting ? 'Importing...' : 'Import'),
                  style: OutlinedButton.styleFrom(
                    foregroundColor: Colors.blue,
                  ),
                ),
              ),
            ],
          ),
          if (saveError != null) ...[
            const SizedBox(height: 12),
            ErrorMessageContainer(message: saveError!),
          ],
          if (loadError != null) ...[
            const SizedBox(height: 12),
            ErrorMessageContainer(message: loadError!),
          ],
          if (exportError != null) ...[
            const SizedBox(height: 12),
            ErrorMessageContainer(message: exportError!),
          ],
          if (importError != null) ...[
            const SizedBox(height: 12),
            ErrorMessageContainer(message: importError!),
          ],
        ],
      ),
    );
  }
}
