import 'package:flutter/widgets.dart';
import 'wallet_state.dart';
import 'output_state.dart';
import 'scan_state.dart';
import 'transaction_state.dart';
import 'file_management_state.dart';

class AppStateScope extends InheritedWidget {
  final WalletState walletState;
  final OutputState outputState;
  final ScanState scanState;
  final TransactionState transactionState;
  final FileManagementState fileManagementState;

  const AppStateScope({
    super.key,
    required this.walletState,
    required this.outputState,
    required this.scanState,
    required this.transactionState,
    required this.fileManagementState,
    required super.child,
  });

  static AppStateScope of(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<AppStateScope>()!;

  static WalletState walletOf(BuildContext context) => of(context).walletState;
  static OutputState outputOf(BuildContext context) => of(context).outputState;
  static ScanState scanOf(BuildContext context) => of(context).scanState;
  static TransactionState transactionOf(BuildContext context) => of(context).transactionState;
  static FileManagementState fileManagementOf(BuildContext context) => of(context).fileManagementState;

  @override
  bool updateShouldNotify(AppStateScope old) => false;
}
