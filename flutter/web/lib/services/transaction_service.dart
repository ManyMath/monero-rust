import '../src/bindings/bindings.dart';
import '../utils/key_parser.dart';
import '../utils/network_utils.dart';
import '../utils/output_lock_utils.dart';

/// Service for handling transaction creation and broadcasting operations
class TransactionService {
  /// Validate transaction creation parameters
  static TransactionCreateValidation validateTransactionCreation({
    required String seed,
    required List<OwnedOutput> availableOutputs,
    required List<RecipientInput> recipients,
    required String nodeUrl,
    Set<String>? selectedOutputs,
    int currentHeight = 0,
  }) {
    // Validate seed phrase
    final result = KeyParser.parse(seed);
    if (!result.isValid || result.normalizedInput == null) {
      return TransactionCreateValidation.error('Please enter a valid seed phrase first');
    }

    // Check if outputs are available
    if (availableOutputs.isEmpty) {
      return TransactionCreateValidation.error(
        'No outputs available. Scan blocks to find outputs first.',
      );
    }

    // Validate all recipients
    List<Recipient> validatedRecipients = [];
    int totalAtomic = 0;

    for (int i = 0; i < recipients.length; i++) {
      final recipient = recipients[i];
      final destination = recipient.address.trim();

      if (destination.isEmpty) {
        return TransactionCreateValidation.error(
          'Please enter a destination address for recipient ${i + 1}',
        );
      }

      final amountStr = recipient.amount.trim();
      if (amountStr.isEmpty) {
        return TransactionCreateValidation.error(
          'Please enter an amount for recipient ${i + 1}',
        );
      }

      final amountXmr = double.tryParse(amountStr);
      if (amountXmr == null || amountXmr <= 0) {
        return TransactionCreateValidation.error(
          'Please enter a valid amount for recipient ${i + 1}',
        );
      }

      // Convert XMR to atomic units (1 XMR = 1e12 atomic units)
      final amountAtomic = (amountXmr * 1e12).round();
      if (amountAtomic <= 0) {
        return TransactionCreateValidation.error(
          'Amount too small for recipient ${i + 1}',
        );
      }

      totalAtomic += amountAtomic;
      validatedRecipients.add(Recipient(
        address: destination,
        amount: Uint64(BigInt.from(amountAtomic)),
      ));
    }

    // Validate coin selection if outputs are selected
    if (selectedOutputs != null && selectedOutputs.isNotEmpty) {
      final selectedTotal = _getSelectedOutputsTotal(
        availableOutputs,
        selectedOutputs,
        currentHeight,
      );
      if (selectedTotal < totalAtomic) {
        final selectedXmr = (selectedTotal / 1e12).toStringAsFixed(12);
        final totalXmr = (totalAtomic / 1e12).toStringAsFixed(12);
        return TransactionCreateValidation.error(
          'Selected outputs ($selectedXmr XMR) insufficient for $totalXmr XMR + fees',
        );
      }
    }

    // Validate node URL
    if (nodeUrl.trim().isEmpty) {
      return TransactionCreateValidation.error('Please enter a node URL');
    }

    return TransactionCreateValidation.success(
      normalizedSeed: result.normalizedInput!,
      recipients: validatedRecipients,
      nodeUrl: NetworkUtils.normalizeNodeUrl(nodeUrl),
      selectedOutputs: selectedOutputs?.toList(),
    );
  }

  /// Create a transaction
  static void createTransaction({
    required String seed,
    required String network,
    required List<Recipient> recipients,
    required String nodeUrl,
    List<String>? selectedOutputs,
  }) {
    CreateTransactionRequest(
      nodeUrl: nodeUrl,
      seed: seed,
      network: network,
      recipients: recipients,
      selectedOutputs: selectedOutputs,
      passphrase: '',
      bip39AccountIndex: 0,
    ).sendSignalToRust();
  }

  /// Validate sweep all transaction parameters
  static SweepAllValidation validateSweepAll({
    required String seed,
    required List<OwnedOutput> availableOutputs,
    required String destinationAddress,
    required String nodeUrl,
    Set<String>? selectedOutputs,
    int currentHeight = 0,
  }) {
    // Validate seed phrase
    final result = KeyParser.parse(seed);
    if (!result.isValid || result.normalizedInput == null) {
      return SweepAllValidation.error('Please enter a valid seed phrase first');
    }

    // Check if outputs are available
    if (availableOutputs.isEmpty) {
      return SweepAllValidation.error(
        'No outputs available. Scan blocks to find outputs first.',
      );
    }

    // Validate destination address
    final destination = destinationAddress.trim();
    if (destination.isEmpty) {
      return SweepAllValidation.error('Please enter a destination address');
    }

    // Validate node URL
    if (nodeUrl.trim().isEmpty) {
      return SweepAllValidation.error('Please enter a node URL');
    }

    // Calculate total amount that will be swept
    final sweepTotal = _getSelectedOutputsTotal(
      availableOutputs,
      selectedOutputs ?? availableOutputs.map((o) => '${o.txHash}:${o.outputIndex}').toSet(),
      currentHeight,
    );

    if (sweepTotal == 0) {
      return SweepAllValidation.error(
        'No spendable outputs available (outputs need 10 confirmations)',
      );
    }

    return SweepAllValidation.success(
      normalizedSeed: result.normalizedInput!,
      destinationAddress: destination,
      nodeUrl: NetworkUtils.normalizeNodeUrl(nodeUrl),
      selectedOutputs: selectedOutputs?.toList(),
      totalAmount: sweepTotal,
    );
  }

  /// Sweep all funds to a destination address
  static void sweepAll({
    required String seed,
    required String network,
    required String destinationAddress,
    required String nodeUrl,
    List<String>? selectedOutputs,
  }) {
    SweepAllRequest(
      nodeUrl: nodeUrl,
      seed: seed,
      network: network,
      destinationAddress: destinationAddress,
      selectedOutputs: selectedOutputs,
      passphrase: '',
      bip39AccountIndex: 0,
    ).sendSignalToRust();
  }

  /// Validate transaction broadcasting parameters
  static TransactionBroadcastValidation validateTransactionBroadcast({
    required TransactionCreatedResponse? txResult,
    required String nodeUrl,
  }) {
    if (txResult == null || txResult.txBlob == null) {
      return TransactionBroadcastValidation.error('No transaction to broadcast');
    }

    if (nodeUrl.trim().isEmpty) {
      return TransactionBroadcastValidation.error('Please enter a node URL');
    }

    return TransactionBroadcastValidation.success(
      txBlob: txResult.txBlob!,
      spentOutputHashes: txResult.spentOutputHashes,
      nodeUrl: NetworkUtils.normalizeNodeUrl(nodeUrl),
    );
  }

  /// Broadcast a transaction
  static void broadcastTransaction({
    required String nodeUrl,
    required String txBlob,
    required List<String> spentOutputHashes,
  }) {
    BroadcastTransactionRequest(
      nodeUrl: nodeUrl,
      txBlob: txBlob,
      spentOutputHashes: spentOutputHashes,
    ).sendSignalToRust();
  }

  /// Calculate maximum spendable amount (total - estimated fee)
  static double calculateMaxSpendable({
    required List<OwnedOutput> availableOutputs,
    Set<String>? selectedOutputs,
    int currentHeight = 0,
  }) {
    final outputKeys = selectedOutputs ??
        availableOutputs.map((o) => '${o.txHash}:${o.outputIndex}').toSet();

    final totalAtomic = _getSelectedOutputsTotal(
      availableOutputs,
      outputKeys,
      currentHeight,
    );

    if (totalAtomic == 0) return 0.0;

    const feePerInputEstimate = 15000000;
    const baseFeeEstimate = 20000000;
    final numInputs = outputKeys.length;
    final estimatedFee = baseFeeEstimate + (numInputs * feePerInputEstimate);

    final maxSpendable = totalAtomic - estimatedFee;
    if (maxSpendable <= 0) return 0.0;

    return maxSpendable / 1e12;
  }

  /// Calculate total of selected outputs
  static int _getSelectedOutputsTotal(
    List<OwnedOutput> allOutputs,
    Set<String> selectedOutputs,
    int currentHeight,
  ) {
    int total = 0;
    for (final output in allOutputs) {
      final outputKey = '${output.txHash}:${output.outputIndex}';
      if (selectedOutputs.contains(outputKey)) {
        if (OutputLockUtils.isOutputSpendable(
          output: output,
          currentHeight: currentHeight,
        )) {
          total += output.amount.toInt();
        }
      }
    }
    return total;
  }
}

/// Input data for a transaction recipient (from UI)
class RecipientInput {
  final String address;
  final String amount;

  const RecipientInput({
    required this.address,
    required this.amount,
  });
}

/// Validation result for transaction creation
class TransactionCreateValidation {
  final bool isValid;
  final String? error;
  final String? normalizedSeed;
  final List<Recipient>? recipients;
  final String? nodeUrl;
  final List<String>? selectedOutputs;

  TransactionCreateValidation._({
    required this.isValid,
    this.error,
    this.normalizedSeed,
    this.recipients,
    this.nodeUrl,
    this.selectedOutputs,
  });

  factory TransactionCreateValidation.success({
    required String normalizedSeed,
    required List<Recipient> recipients,
    required String nodeUrl,
    List<String>? selectedOutputs,
  }) {
    return TransactionCreateValidation._(
      isValid: true,
      normalizedSeed: normalizedSeed,
      recipients: recipients,
      nodeUrl: nodeUrl,
      selectedOutputs: selectedOutputs,
    );
  }

  factory TransactionCreateValidation.error(String error) {
    return TransactionCreateValidation._(
      isValid: false,
      error: error,
    );
  }
}

/// Validation result for transaction broadcast
class TransactionBroadcastValidation {
  final bool isValid;
  final String? error;
  final String? txBlob;
  final List<String>? spentOutputHashes;
  final String? nodeUrl;

  TransactionBroadcastValidation._({
    required this.isValid,
    this.error,
    this.txBlob,
    this.spentOutputHashes,
    this.nodeUrl,
  });

  factory TransactionBroadcastValidation.success({
    required String txBlob,
    required List<String> spentOutputHashes,
    required String nodeUrl,
  }) {
    return TransactionBroadcastValidation._(
      isValid: true,
      txBlob: txBlob,
      spentOutputHashes: spentOutputHashes,
      nodeUrl: nodeUrl,
    );
  }

  factory TransactionBroadcastValidation.error(String error) {
    return TransactionBroadcastValidation._(
      isValid: false,
      error: error,
    );
  }
}

/// Validation result for sweep all operation
class SweepAllValidation {
  final bool isValid;
  final String? error;
  final String? normalizedSeed;
  final String? destinationAddress;
  final String? nodeUrl;
  final List<String>? selectedOutputs;
  final int? totalAmount;

  SweepAllValidation._({
    required this.isValid,
    this.error,
    this.normalizedSeed,
    this.destinationAddress,
    this.nodeUrl,
    this.selectedOutputs,
    this.totalAmount,
  });

  factory SweepAllValidation.success({
    required String normalizedSeed,
    required String destinationAddress,
    required String nodeUrl,
    List<String>? selectedOutputs,
    required int totalAmount,
  }) {
    return SweepAllValidation._(
      isValid: true,
      normalizedSeed: normalizedSeed,
      destinationAddress: destinationAddress,
      nodeUrl: nodeUrl,
      selectedOutputs: selectedOutputs,
      totalAmount: totalAmount,
    );
  }

  factory SweepAllValidation.error(String error) {
    return SweepAllValidation._(
      isValid: false,
      error: error,
    );
  }
}
