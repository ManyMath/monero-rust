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
  final bool isViewOnly;
  final bool isExportingKeyImages;
  final bool isImportingKeyImages;
  final String? saveError;
  final String? loadError;
  final String? exportError;
  final String? importError;
  final String? keyImageExportError;
  final String? keyImageImportError;
  final String? seed;
  final int transactionCount;
  final int outputCount;
  final Function(String) onWalletChanged;
  final VoidCallback onLoad;
  final VoidCallback onDelete;
  final VoidCallback onSave;
  final VoidCallback onNew;
  final VoidCallback onExport;
  final VoidCallback onImport;
  final VoidCallback onExportKeyImages;
  final VoidCallback onImportKeyImages;
  final Function(String) onCloseWallet;
  final bool autoSaveEnabled;
  final ValueChanged<bool> onAutoSaveChanged;

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
    required this.isViewOnly,
    required this.isExportingKeyImages,
    required this.isImportingKeyImages,
    required this.saveError,
    required this.loadError,
    required this.exportError,
    required this.importError,
    required this.keyImageExportError,
    required this.keyImageImportError,
    required this.seed,
    required this.transactionCount,
    required this.outputCount,
    required this.onWalletChanged,
    required this.onLoad,
    required this.onDelete,
    required this.onSave,
    required this.onNew,
    required this.onExport,
    required this.onImport,
    required this.onExportKeyImages,
    required this.onImportKeyImages,
    required this.onCloseWallet,
    required this.autoSaveEnabled,
    required this.onAutoSaveChanged,
  });

  bool get hasWalletData =>
      (seed?.trim().isNotEmpty ?? false) ||
      transactionCount > 0 ||
      outputCount > 0;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16.0),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (availableWalletIds.isNotEmpty || hasWalletData) ...[
            WalletSwitcher(
              availableWalletIds: availableWalletIds,
              walletId: walletId,
              lastSaveTime: lastSaveTime,
              isLoadingWallet: isLoadingWallet,
              isSaving: isSaving,
              seed: seed,
              transactionCount: transactionCount,
              outputCount: outputCount,
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
              if (hasWalletData) ...[
                const SizedBox(width: 8),
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
              ],
            ],
          ),
          if (hasWalletData) ...[
            const SizedBox(height: 12),
            Row(
              children: [
                if (!isViewOnly) ...[
                  Expanded(
                    child: OutlinedButton.icon(
                      onPressed: isExportingKeyImages ? null : onExportKeyImages,
                      icon: isExportingKeyImages
                          ? const SizedBox(
                              width: 16,
                              height: 16,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Icon(Icons.key),
                      label: Text(
                        isExportingKeyImages
                            ? 'Exporting...'
                            : 'Export Key Images',
                      ),
                      style: OutlinedButton.styleFrom(
                        foregroundColor: Colors.teal,
                      ),
                    ),
                  ),
                ] else ...[
                  Expanded(
                    child: OutlinedButton.icon(
                      onPressed: isImportingKeyImages ? null : onImportKeyImages,
                      icon: isImportingKeyImages
                          ? const SizedBox(
                              width: 16,
                              height: 16,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Icon(Icons.key),
                      label: Text(
                        isImportingKeyImages
                            ? 'Importing...'
                            : 'Import Key Images',
                      ),
                      style: OutlinedButton.styleFrom(
                        foregroundColor: Colors.teal,
                      ),
                    ),
                  ),
                ],
              ],
            ),
            const SizedBox(height: 12),
            Row(
              children: [
                SizedBox(
                  height: 24,
                  width: 24,
                  child: Checkbox(
                    value: autoSaveEnabled,
                    onChanged: (v) => onAutoSaveChanged(v ?? false),
                  ),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: GestureDetector(
                    onTap: () => onAutoSaveChanged(!autoSaveEnabled),
                    child: Text(
                      'Auto-save (every 2 min & after each transaction)',
                      style: TextStyle(
                        fontSize: 12,
                        color: Colors.grey[400],
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ],
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
          if (keyImageExportError != null) ...[
            const SizedBox(height: 12),
            ErrorMessageContainer(message: keyImageExportError!),
          ],
          if (keyImageImportError != null) ...[
            const SizedBox(height: 12),
            ErrorMessageContainer(message: keyImageImportError!),
          ],
        ],
      ),
    );
  }
}
