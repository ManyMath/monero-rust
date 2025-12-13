import 'package:flutter_test/flutter_test.dart';
import 'package:monero_extension/src/bindings/bindings.dart';
import 'package:monero_extension/utils/output_utils.dart';
import '../test_helpers.dart';

ChangeOutput createMockChangeOutput({
  String txHash = 'change_tx',
  int outputIndex = 0,
  String amountXmr = '1.5',
  String? keyImage,
  (int, int)? subaddressIndex,
}) {
  final amount = (double.parse(amountXmr) * 1e12).toInt();
  return ChangeOutput(
    txHash: txHash,
    outputIndex: outputIndex,
    amount: amount,
    amountXmr: amountXmr,
    key: 'key_$txHash',
    keyOffset: 'offset_$txHash',
    commitmentMask: 'mask_$txHash',
    subaddressIndex: subaddressIndex,
    receivedOutputBytes: 'bytes_$txHash',
    keyImage: keyImage ?? 'ki_$txHash',
  );
}

void main() {
  group('mergeScannedOutputs', () {
    test('adds new output not in list', () {
      final existing = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100),
      ];
      final incoming = [
        TestHelpers.createMockOutput(txHash: 'b', outputIndex: 0, amountXmr: '2.0', blockHeight: 200),
      ];

      OutputUtils.mergeScannedOutputs(existing, incoming);

      expect(existing.length, 2);
      expect(existing[1].txHash, 'b');
    });

    test('updates unconfirmed output with confirmed version', () {
      final existing = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 0),
      ];
      final incoming = [
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 500),
      ];

      OutputUtils.mergeScannedOutputs(existing, incoming);

      expect(existing.length, 1);
      expect(existing[0].blockHeight.toInt(), 500);
    });

    test('skips confirmed duplicate', () {
      final existing = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100),
      ];
      final incoming = [
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 200),
      ];

      OutputUtils.mergeScannedOutputs(existing, incoming);

      expect(existing.length, 1);
      expect(existing[0].blockHeight.toInt(), 100, reason: 'Should keep original confirmed output');
    });

    test('handles empty incoming', () {
      final existing = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100),
      ];

      OutputUtils.mergeScannedOutputs(existing, []);

      expect(existing.length, 1);
    });

    test('mixed: adds new + updates unconfirmed + skips confirmed', () {
      final existing = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'confirmed', outputIndex: 0, amountXmr: '1.0', blockHeight: 100),
        TestHelpers.createMockOutput(txHash: 'unconfirmed', outputIndex: 0, amountXmr: '2.0', blockHeight: 0),
      ];
      final incoming = [
        TestHelpers.createMockOutput(txHash: 'confirmed', outputIndex: 0, amountXmr: '1.0', blockHeight: 200),
        TestHelpers.createMockOutput(txHash: 'unconfirmed', outputIndex: 0, amountXmr: '2.0', blockHeight: 300),
        TestHelpers.createMockOutput(txHash: 'brand_new', outputIndex: 0, amountXmr: '3.0', blockHeight: 400),
      ];

      OutputUtils.mergeScannedOutputs(existing, incoming);

      expect(existing.length, 3);
      expect(existing[0].blockHeight.toInt(), 100, reason: 'Confirmed kept');
      expect(existing[1].blockHeight.toInt(), 300, reason: 'Unconfirmed updated');
      expect(existing[2].txHash, 'brand_new', reason: 'New added');
    });
  });

  group('addIfAbsent', () {
    test('adds output not in list', () {
      final existing = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100),
      ];
      final incoming = [
        TestHelpers.createMockOutput(txHash: 'b', outputIndex: 0, amountXmr: '2.0', blockHeight: 200),
      ];

      OutputUtils.addIfAbsent(existing, incoming);

      expect(existing.length, 2);
      expect(existing[1].txHash, 'b');
    });

    test('skips output already in list', () {
      final existing = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100),
      ];
      final incoming = [
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 200),
      ];

      OutputUtils.addIfAbsent(existing, incoming);

      expect(existing.length, 1);
      expect(existing[0].blockHeight.toInt(), 100, reason: 'Original preserved');
    });

    test('mixed: adds some, skips others', () {
      final existing = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100),
      ];
      final incoming = [
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 200),
        TestHelpers.createMockOutput(txHash: 'b', outputIndex: 0, amountXmr: '3.0', blockHeight: 300),
      ];

      OutputUtils.addIfAbsent(existing, incoming);

      expect(existing.length, 2);
      expect(existing[0].txHash, 'a');
      expect(existing[1].txHash, 'b');
    });

    test('handles empty incoming', () {
      final existing = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100),
      ];

      OutputUtils.addIfAbsent(existing, []);

      expect(existing.length, 1);
    });
  });

  group('changeOutputToOwned', () {
    test('maps all fields correctly', () {
      final change = createMockChangeOutput(
        txHash: 'tx123',
        outputIndex: 2,
        amountXmr: '5.0',
        keyImage: 'ki_test',
      );

      final owned = OutputUtils.changeOutputToOwned(change);

      expect(owned.txHash, 'tx123');
      expect(owned.outputIndex, 2);
      expect(owned.amountXmr, '5.0');
      expect(owned.key, 'key_tx123');
      expect(owned.keyOffset, 'offset_tx123');
      expect(owned.commitmentMask, 'mask_tx123');
      expect(owned.receivedOutputBytes, 'bytes_tx123');
      expect(owned.keyImage, 'ki_test');
    });

    test('sets blockHeight=0, spent=false, paymentId=null', () {
      final change = createMockChangeOutput();

      final owned = OutputUtils.changeOutputToOwned(change);

      expect(owned.blockHeight.toInt(), 0);
      expect(owned.spent, false);
      expect(owned.paymentId, isNull);
    });

    test('preserves nullable subaddressIndex', () {
      final withIndex = createMockChangeOutput(
        subaddressIndex: (0, 1),
      );
      final withoutIndex = createMockChangeOutput(subaddressIndex: null);

      expect(OutputUtils.changeOutputToOwned(withIndex).subaddressIndex, (0, 1));
      expect(OutputUtils.changeOutputToOwned(withoutIndex).subaddressIndex, isNull);
    });
  });

  group('markSpentByKeyImages', () {
    test('marks matching output as spent', () {
      final outputs = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100, keyImage: 'ki_a'),
        TestHelpers.createMockOutput(txHash: 'b', outputIndex: 0, amountXmr: '2.0', blockHeight: 200, keyImage: 'ki_b'),
      ];
      final selected = <String>{'a:0', 'b:0'};

      OutputUtils.markSpentByKeyImages(outputs, ['ki_a'], selected);

      expect(outputs[0].spent, true);
      expect(outputs[1].spent, false);
    });

    test('removes spent output from selectedOutputs', () {
      final outputs = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100, keyImage: 'ki_a'),
      ];
      final selected = <String>{'a:0'};

      OutputUtils.markSpentByKeyImages(outputs, ['ki_a'], selected);

      expect(selected, isEmpty);
    });

    test('skips already-spent outputs', () {
      final outputs = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100, keyImage: 'ki_a', spent: true),
      ];
      final selected = <String>{'a:0'};

      OutputUtils.markSpentByKeyImages(outputs, ['ki_a'], selected);

      expect(selected, {'a:0'}, reason: 'Already-spent output should not remove from selection again');
    });

    test('no matches is a no-op', () {
      final outputs = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100, keyImage: 'ki_a'),
      ];
      final selected = <String>{'a:0'};

      OutputUtils.markSpentByKeyImages(outputs, ['ki_unknown'], selected);

      expect(outputs[0].spent, false);
      expect(selected, {'a:0'});
    });

    test('multiple key images in one call', () {
      final outputs = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100, keyImage: 'ki_a'),
        TestHelpers.createMockOutput(txHash: 'b', outputIndex: 0, amountXmr: '2.0', blockHeight: 200, keyImage: 'ki_b'),
        TestHelpers.createMockOutput(txHash: 'c', outputIndex: 0, amountXmr: '3.0', blockHeight: 300, keyImage: 'ki_c'),
      ];
      final selected = <String>{'a:0', 'b:0', 'c:0'};

      OutputUtils.markSpentByKeyImages(outputs, ['ki_a', 'ki_c'], selected);

      expect(outputs[0].spent, true);
      expect(outputs[1].spent, false);
      expect(outputs[2].spent, true);
      expect(selected, {'b:0'});
    });
  });

  group('markSpentByOutputKeys', () {
    test('marks matching output as spent', () {
      final outputs = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100),
        TestHelpers.createMockOutput(txHash: 'b', outputIndex: 0, amountXmr: '2.0', blockHeight: 200),
      ];
      final selected = <String>{'a:0', 'b:0'};

      OutputUtils.markSpentByOutputKeys(outputs, ['a:0'], selected);

      expect(outputs[0].spent, true);
      expect(outputs[1].spent, false);
    });

    test('removes from selectedOutputs', () {
      final outputs = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100),
      ];
      final selected = <String>{'a:0'};

      OutputUtils.markSpentByOutputKeys(outputs, ['a:0'], selected);

      expect(selected, isEmpty);
    });

    test('no matches is a no-op', () {
      final outputs = <OwnedOutput>[
        TestHelpers.createMockOutput(txHash: 'a', outputIndex: 0, amountXmr: '1.0', blockHeight: 100),
      ];
      final selected = <String>{'a:0'};

      OutputUtils.markSpentByOutputKeys(outputs, ['unknown:0'], selected);

      expect(outputs[0].spent, false);
      expect(selected, {'a:0'});
    });
  });
}
