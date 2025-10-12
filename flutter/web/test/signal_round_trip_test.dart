import 'dart:convert';
import 'package:test/test.dart';
import 'package:monero_extension/src/ffi/signal_types.dart';

void main() {
  // -----------------------------------------------------------------------
  // Sub-struct round-trips
  // -----------------------------------------------------------------------

  group('Recipient round-trip', () {
    test('basic fields', () {
      final original = Recipient(address: '5addr...abc', amount: 1000000000000);
      final json = original.toJson();
      final restored = Recipient.fromJson(json);

      expect(restored.address, original.address);
      expect(restored.amount, original.amount);
    });

    test('large amount near u64 max (safe JS integer range)', () {
      // 2^53 - 1 is the max safe integer in JS / Dart's double-backed int
      final original = Recipient(
        address: '5bigaddr',
        amount: 9007199254740992, // 2^53
      );
      final json = original.toJson();
      final decoded = Recipient.fromJson(json);

      expect(decoded.amount, original.amount);
    });
  });

  group('OwnedOutput round-trip', () {
    test('all required fields, no optionals', () {
      final original = OwnedOutput(
        txHash: 'aabbccdd' * 8,
        outputIndex: 0,
        amount: 5000000000000,
        amountXmr: '5.000000000000',
        key: 'key_hex_' * 4,
        keyOffset: 'offset_hex_' * 3,
        commitmentMask: 'mask_hex_' * 4,
        receivedOutputBytes: 'received_bytes_hex',
        blockHeight: 1384526,
        spent: false,
        keyImage: 'ki_hex_' * 9,
        isCoinbase: false,
        frozen: false,
      );

      final json = original.toJson();
      final restored = OwnedOutput.fromJson(json);

      expect(restored.txHash, original.txHash);
      expect(restored.outputIndex, original.outputIndex);
      expect(restored.amount, original.amount);
      expect(restored.amountXmr, original.amountXmr);
      expect(restored.key, original.key);
      expect(restored.keyOffset, original.keyOffset);
      expect(restored.commitmentMask, original.commitmentMask);
      expect(restored.subaddressIndex, isNull);
      expect(restored.paymentId, isNull);
      expect(restored.receivedOutputBytes, original.receivedOutputBytes);
      expect(restored.blockHeight, original.blockHeight);
      expect(restored.spent, false);
      expect(restored.keyImage, original.keyImage);
      expect(restored.isCoinbase, false);
      expect(restored.frozen, false);
    });

    test('with subaddressIndex tuple', () {
      final original = OwnedOutput(
        txHash: 'tx1',
        outputIndex: 1,
        amount: 100,
        amountXmr: '0.000000000100',
        key: 'k',
        keyOffset: 'o',
        commitmentMask: 'm',
        subaddressIndex: (2, 5),
        receivedOutputBytes: 'b',
        blockHeight: 999,
        spent: true,
        keyImage: 'ki',
        isCoinbase: true,
        frozen: true,
      );

      final json = original.toJson();
      // Verify the tuple serializes as a two-element list
      expect(json['subaddress_index'], [2, 5]);

      final restored = OwnedOutput.fromJson(json);
      expect(restored.subaddressIndex, (2, 5));
      expect(restored.spent, true);
      expect(restored.isCoinbase, true);
      expect(restored.frozen, true);
    });

    test('with paymentId', () {
      final original = OwnedOutput(
        txHash: 'tx2',
        outputIndex: 0,
        amount: 200,
        amountXmr: '0.000000000200',
        key: 'k',
        keyOffset: 'o',
        commitmentMask: 'm',
        paymentId: 'deadbeef12345678',
        receivedOutputBytes: 'b',
        blockHeight: 500,
        spent: false,
        keyImage: 'ki',
        isCoinbase: false,
        frozen: false,
      );

      final json = original.toJson();
      expect(json['payment_id'], 'deadbeef12345678');

      final restored = OwnedOutput.fromJson(json);
      expect(restored.paymentId, 'deadbeef12345678');
    });

    test('null optional fields are absent from JSON', () {
      final original = OwnedOutput(
        txHash: 'tx3',
        outputIndex: 0,
        amount: 0,
        amountXmr: '0.000000000000',
        key: 'k',
        keyOffset: 'o',
        commitmentMask: 'm',
        receivedOutputBytes: 'b',
        blockHeight: 0,
        spent: false,
        keyImage: 'ki',
        isCoinbase: false,
        frozen: false,
      );

      final json = original.toJson();
      expect(json.containsKey('subaddress_index'), false);
      expect(json.containsKey('payment_id'), false);
    });

    test('JSON string encode/decode survives', () {
      final original = OwnedOutput(
        txHash: 'tx_json',
        outputIndex: 3,
        amount: 999999999999,
        amountXmr: '0.999999999999',
        key: 'k',
        keyOffset: 'o',
        commitmentMask: 'm',
        subaddressIndex: (0, 0),
        receivedOutputBytes: 'b',
        blockHeight: 42,
        spent: false,
        keyImage: 'ki',
        isCoinbase: false,
        frozen: false,
      );

      final jsonString = jsonEncode(original.toJson());
      final decoded = jsonDecode(jsonString) as Map<String, dynamic>;
      final restored = OwnedOutput.fromJson(decoded);

      expect(restored.txHash, 'tx_json');
      expect(restored.amount, 999999999999);
      expect(restored.subaddressIndex, (0, 0));
    });
  });

  group('ChangeOutput round-trip', () {
    test('with subaddressIndex', () {
      final original = ChangeOutput(
        txHash: 'change_tx',
        outputIndex: 1,
        amount: 500000,
        amountXmr: '0.000000500000',
        key: 'ck',
        keyOffset: 'co',
        commitmentMask: 'cm',
        subaddressIndex: (0, 1),
        receivedOutputBytes: 'cb',
        keyImage: 'cki',
      );

      final json = original.toJson();
      final restored = ChangeOutput.fromJson(json);

      expect(restored.txHash, 'change_tx');
      expect(restored.outputIndex, 1);
      expect(restored.amount, 500000);
      expect(restored.subaddressIndex, (0, 1));
      expect(restored.keyImage, 'cki');
    });

    test('without subaddressIndex', () {
      final original = ChangeOutput(
        txHash: 'change_tx2',
        outputIndex: 0,
        amount: 100,
        amountXmr: '0.000000000100',
        key: 'ck',
        keyOffset: 'co',
        commitmentMask: 'cm',
        receivedOutputBytes: 'cb',
        keyImage: 'cki2',
      );

      final json = original.toJson();
      expect(json.containsKey('subaddress_index'), false);

      final restored = ChangeOutput.fromJson(json);
      expect(restored.subaddressIndex, isNull);
    });
  });

  group('DoubleSpendConflict round-trip', () {
    test('basic fields', () {
      final original = DoubleSpendConflict(
        keyImage: 'ki_conflict',
        previousSpentHeight: 1000,
        newHeight: 1005,
      );
      final json = original.toJson();
      final restored = DoubleSpendConflict.fromJson(json);

      expect(restored.keyImage, 'ki_conflict');
      expect(restored.previousSpentHeight, 1000);
      expect(restored.newHeight, 1005);
    });
  });

  group('WalletScanResult round-trip', () {
    test('with nested outputs', () {
      final output = OwnedOutput(
        txHash: 'scan_tx',
        outputIndex: 0,
        amount: 7777,
        amountXmr: '0.000000007777',
        key: 'k',
        keyOffset: 'o',
        commitmentMask: 'm',
        receivedOutputBytes: 'b',
        blockHeight: 100,
        spent: false,
        keyImage: 'ki',
        isCoinbase: false,
        frozen: false,
      );

      final original = WalletScanResult(
        address: '5scanaddr',
        outputs: [output],
      );

      final json = original.toJson();
      final restored = WalletScanResult.fromJson(json);

      expect(restored.address, '5scanaddr');
      expect(restored.outputs.length, 1);
      expect(restored.outputs[0].txHash, 'scan_tx');
      expect(restored.outputs[0].amount, 7777);
    });

    test('with empty outputs list', () {
      final original = WalletScanResult(address: '5empty', outputs: []);
      final json = original.toJson();
      final restored = WalletScanResult.fromJson(json);

      expect(restored.outputs, isEmpty);
    });
  });

  // -----------------------------------------------------------------------
  // DartSignal types: construct -> toJson (via sendSignalToRust internals)
  // We test the JSON map that would be sent, not the send itself.
  //
  // LIMITATION: These tests manually reconstruct the JSON map rather than
  // calling sendSignalToRust(), so drift between the test and the real
  // serialization code could go undetected. A future improvement would be
  // to extract toJson() methods on each DartSignal class and test those
  // directly (see also: RustSignal classes which already have fromJson()).
  // -----------------------------------------------------------------------

  group('DartSignal: CreateTransactionRequest', () {
    test('with optional fields populated', () {
      final req = CreateTransactionRequest(
        nodeUrl: 'http://localhost:38081',
        seed: 'test seed words here',
        network: 'stagenet',
        recipients: [
          Recipient(address: '5addr1', amount: 1000000000000),
          Recipient(address: '5addr2', amount: 2000000000000),
        ],
        selectedOutputs: ['txhash1:0', 'txhash2:1'],
        passphrase: 'mypass',
        bip39AccountIndex: 2,
        subtractFee: true,
      );

      // Build the JSON map the same way sendSignalToRust does
      final json = {
        'node_url': req.nodeUrl,
        'seed': req.seed,
        'network': req.network,
        'recipients': req.recipients.map((e) => e.toJson()).toList(),
        if (req.selectedOutputs != null) 'selected_outputs': req.selectedOutputs,
        'passphrase': req.passphrase,
        'bip39_account_index': req.bip39AccountIndex,
        'subtract_fee': req.subtractFee,
      };

      expect(json['node_url'], 'http://localhost:38081');
      expect((json['recipients'] as List).length, 2);
      expect(json['selected_outputs'], ['txhash1:0', 'txhash2:1']);
      expect(json['subtract_fee'], true);
      expect(json['bip39_account_index'], 2);
    });

    test('without optional selectedOutputs', () {
      final req = CreateTransactionRequest(
        nodeUrl: 'http://node:38081',
        seed: 'seed',
        network: 'mainnet',
        recipients: [Recipient(address: 'a', amount: 1)],
      );

      final json = {
        'node_url': req.nodeUrl,
        'seed': req.seed,
        'network': req.network,
        'recipients': req.recipients.map((e) => e.toJson()).toList(),
        if (req.selectedOutputs != null) 'selected_outputs': req.selectedOutputs,
        'passphrase': req.passphrase,
        'bip39_account_index': req.bip39AccountIndex,
        'subtract_fee': req.subtractFee,
      };

      expect(json.containsKey('selected_outputs'), false);
      expect(json['passphrase'], '');
      expect(json['bip39_account_index'], 0);
      expect(json['subtract_fee'], false);
    });
  });

  group('DartSignal: StartContinuousScanRequest', () {
    test('with accountsToScan', () {
      final req = StartContinuousScanRequest(
        nodeUrl: 'http://localhost:38081',
        startHeight: 1000,
        seed: 'seed words',
        network: 'stagenet',
        accountLookahead: 5,
        subaddressLookahead: 10,
        accountsToScan: [0, 1, 2],
        passphrase: 'pass',
        bip39AccountIndex: 1,
      );

      final json = {
        'node_url': req.nodeUrl,
        'start_height': req.startHeight,
        'seed': req.seed,
        'network': req.network,
        'account_lookahead': req.accountLookahead,
        'subaddress_lookahead': req.subaddressLookahead,
        if (req.accountsToScan != null) 'accounts_to_scan': req.accountsToScan,
        'passphrase': req.passphrase,
        'bip39_account_index': req.bip39AccountIndex,
      };

      expect(json['accounts_to_scan'], [0, 1, 2]);
      expect(json['subaddress_lookahead'], 10);
    });
  });

  group('DartSignal: RestoreWalletDataRequest', () {
    test('with nested outputs and optional fields', () {
      final output = OwnedOutput(
        txHash: 'restore_tx',
        outputIndex: 0,
        amount: 100,
        amountXmr: '0.000000000100',
        key: 'k',
        keyOffset: 'o',
        commitmentMask: 'm',
        subaddressIndex: (1, 3),
        receivedOutputBytes: 'b',
        blockHeight: 50,
        spent: false,
        keyImage: 'ki',
        isCoinbase: false,
        frozen: false,
      );

      final req = RestoreWalletDataRequest(
        seed: 'test seed',
        network: 'stagenet',
        outputs: [output],
        daemonHeight: 2000,
        currentHeight: 1500,
        blockHashesJson: '{"hashes":["abc"]}',
        pendingStateJson: '{"pending":[]}',
        passphrase: 'pw',
        bip39AccountIndex: 3,
      );

      final json = {
        'seed': req.seed,
        'network': req.network,
        'outputs': req.outputs.map((e) => e.toJson()).toList(),
        'daemon_height': req.daemonHeight,
        'current_height': req.currentHeight,
        if (req.blockHashesJson != null) 'block_hashes_json': req.blockHashesJson,
        if (req.pendingStateJson != null) 'pending_state_json': req.pendingStateJson,
        'passphrase': req.passphrase,
        'bip39_account_index': req.bip39AccountIndex,
      };

      expect((json['outputs'] as List).length, 1);
      final outputJson = (json['outputs'] as List)[0] as Map<String, dynamic>;
      expect(outputJson['subaddress_index'], [1, 3]);
      expect(json['block_hashes_json'], '{"hashes":["abc"]}');
      expect(json['pending_state_json'], '{"pending":[]}');
    });
  });

  group('DartSignal: BroadcastTransactionRequest', () {
    test('with spent key images', () {
      final req = BroadcastTransactionRequest(
        nodeUrl: 'http://node:38081',
        txBlob: 'deadbeef',
        spentOutputHashes: ['hash1:0', 'hash2:1'],
        txId: 'txid123',
        spentKeyImages: ['ki1', 'ki2', 'ki3'],
        doNotRelay: true,
      );

      final json = {
        'node_url': req.nodeUrl,
        'tx_blob': req.txBlob,
        'spent_output_hashes': req.spentOutputHashes,
        'tx_id': req.txId,
        'spent_key_images': req.spentKeyImages,
        'do_not_relay': req.doNotRelay,
      };

      expect(json['spent_key_images'], ['ki1', 'ki2', 'ki3']);
      expect(json['spent_output_hashes'], ['hash1:0', 'hash2:1']);
      expect(json['do_not_relay'], true);
    });
  });

  group('DartSignal: CreateUnsignedTransactionRequest', () {
    test('offline signing fields', () {
      final req = CreateUnsignedTransactionRequest(
        nodeUrl: 'http://node:38081',
        viewKeyHex: 'aabb' * 16,
        pubSpendKeyHex: 'ccdd' * 16,
        network: 'stagenet',
        recipients: [Recipient(address: '5dest', amount: 5000000000000)],
        selectedOutputs: ['out1:0'],
      );

      final json = {
        'node_url': req.nodeUrl,
        'view_key_hex': req.viewKeyHex,
        'pub_spend_key_hex': req.pubSpendKeyHex,
        'network': req.network,
        'recipients': req.recipients.map((e) => e.toJson()).toList(),
        if (req.selectedOutputs != null) 'selected_outputs': req.selectedOutputs,
      };

      expect(json['view_key_hex'], 'aabb' * 16);
      expect(json['selected_outputs'], ['out1:0']);
    });
  });

  // -----------------------------------------------------------------------
  // RustSignal types: JSON map -> fromJson() -> verify fields
  // -----------------------------------------------------------------------

  group('RustSignal: BlockScanResponse', () {
    test('with outputs and spent key images', () {
      final json = {
        'success': true,
        'error': null,
        'block_height': 1384526,
        'block_hash': 'a5918cf3adadfabee8675011d574aa5cea619d7cedd62a58bd81d391dc4234db',
        'block_timestamp': 1688074142,
        'tx_count': 2,
        'outputs': [
          {
            'tx_hash': '07a561e60118c0a485b20bbfac787fd8efead96a9f422d9dff4a86f2985db7c5',
            'output_index': 0,
            'amount': 10000000000000,
            'amount_xmr': '10.000000000000',
            'key': 'somekey',
            'key_offset': 'someoffset',
            'commitment_mask': 'somemask',
            'subaddress_index': [0, 0],
            'received_output_bytes': 'somebytes',
            'block_height': 1384526,
            'spent': false,
            'key_image': 'somekeyimage',
            'is_coinbase': false,
            'frozen': false,
          }
        ],
        'daemon_height': 2037532,
        'spent_key_images': ['spent_ki_1', 'spent_ki_2'],
        'spent_key_image_tx_hashes': ['spent_tx_1', 'spent_tx_2'],
      };

      final response = BlockScanResponse.fromJson(json);

      expect(response.success, true);
      expect(response.error, isNull);
      expect(response.blockHeight, 1384526);
      expect(response.blockHash, startsWith('a5918cf3'));
      expect(response.blockTimestamp, 1688074142);
      expect(response.txCount, 2);
      expect(response.outputs.length, 1);
      expect(response.outputs[0].txHash, startsWith('07a561'));
      expect(response.outputs[0].amount, 10000000000000);
      expect(response.outputs[0].subaddressIndex, (0, 0));
      expect(response.daemonHeight, 2037532);
      expect(response.spentKeyImages, ['spent_ki_1', 'spent_ki_2']);
      expect(response.spentKeyImageTxHashes, ['spent_tx_1', 'spent_tx_2']);
    });

    test('with empty outputs', () {
      final json = {
        'success': true,
        'block_height': 100,
        'block_hash': 'hash',
        'block_timestamp': 0,
        'tx_count': 0,
        'outputs': [],
        'daemon_height': 200,
        'spent_key_images': [],
        'spent_key_image_tx_hashes': [],
      };

      final response = BlockScanResponse.fromJson(json);
      expect(response.outputs, isEmpty);
      expect(response.spentKeyImages, isEmpty);
    });

    test('with error', () {
      final json = {
        'success': false,
        'error': 'Connection refused',
        'block_height': 0,
        'block_hash': '',
        'block_timestamp': 0,
        'tx_count': 0,
        'outputs': [],
        'daemon_height': 0,
        'spent_key_images': [],
        'spent_key_image_tx_hashes': [],
      };

      final response = BlockScanResponse.fromJson(json);
      expect(response.success, false);
      expect(response.error, 'Connection refused');
    });
  });

  group('RustSignal: TransactionCreatedResponse', () {
    test('successful transaction with change outputs', () {
      final json = {
        'success': true,
        'error': null,
        'tx_id': 'aabb' * 16,
        'fee': 44000000,
        'tx_blob': 'blob_hex_data',
        'tx_key': 'txkey_hex',
        'tx_key_additional': ['extra_key_1', 'extra_key_2'],
        'spent_output_hashes': ['spent1:0', 'spent2:1'],
        'change_outputs': [
          {
            'tx_hash': 'change_tx_hash',
            'output_index': 1,
            'amount': 999956000000,
            'amount_xmr': '0.999956000000',
            'key': 'change_key',
            'key_offset': 'change_offset',
            'commitment_mask': 'change_mask',
            'subaddress_index': [0, 0],
            'received_output_bytes': 'change_bytes',
            'key_image': 'change_ki',
          }
        ],
      };

      final response = TransactionCreatedResponse.fromJson(json);

      expect(response.success, true);
      expect(response.txId, 'aabb' * 16);
      expect(response.fee, 44000000);
      expect(response.txBlob, 'blob_hex_data');
      expect(response.txKey, 'txkey_hex');
      expect(response.txKeyAdditional, ['extra_key_1', 'extra_key_2']);
      expect(response.spentOutputHashes.length, 2);
      expect(response.changeOutputs.length, 1);
      expect(response.changeOutputs[0].subaddressIndex, (0, 0));
    });

    test('failed transaction', () {
      final json = {
        'success': false,
        'error': 'Insufficient funds',
        'tx_id': '',
        'fee': 0,
        'tx_blob': null,
        'tx_key': null,
        'tx_key_additional': [],
        'spent_output_hashes': [],
        'change_outputs': [],
      };

      final response = TransactionCreatedResponse.fromJson(json);
      expect(response.success, false);
      expect(response.error, 'Insufficient funds');
      expect(response.changeOutputs, isEmpty);
    });
  });

  group('RustSignal: MultiWalletScanResponse', () {
    test('with multiple wallet results', () {
      final json = {
        'success': true,
        'error': null,
        'block_height': 5000,
        'block_hash': 'blockhash',
        'block_timestamp': 1700000000,
        'tx_count': 3,
        'daemon_height': 5100,
        'spent_key_images': ['ki1'],
        'spent_key_image_tx_hashes': ['txh1'],
        'wallet_results': [
          {
            'address': '5wallet1addr',
            'outputs': [],
          },
          {
            'address': '5wallet2addr',
            'outputs': [
              {
                'tx_hash': 'multi_tx',
                'output_index': 0,
                'amount': 500,
                'amount_xmr': '0.000000000500',
                'key': 'k',
                'key_offset': 'o',
                'commitment_mask': 'm',
                'received_output_bytes': 'b',
                'block_height': 5000,
                'spent': false,
                'key_image': 'ki',
                'is_coinbase': false,
                'frozen': false,
              }
            ],
          },
        ],
      };

      final response = MultiWalletScanResponse.fromJson(json);

      expect(response.walletResults.length, 2);
      expect(response.walletResults[0].outputs, isEmpty);
      expect(response.walletResults[1].outputs.length, 1);
      expect(response.walletResults[1].address, '5wallet2addr');
    });
  });

  group('RustSignal: SyncProgressResponse', () {
    test('basic fields', () {
      final json = {
        'current_height': 1500,
        'daemon_height': 2000,
        'is_synced': false,
        'is_scanning': true,
      };

      final response = SyncProgressResponse.fromJson(json);
      expect(response.currentHeight, 1500);
      expect(response.daemonHeight, 2000);
      expect(response.isSynced, false);
      expect(response.isScanning, true);
    });
  });

  group('RustSignal: KeysDerivedResponse', () {
    test('all key fields', () {
      final json = {
        'address': '5addr...',
        'secret_spend_key': 'ssk_hex',
        'secret_view_key': 'svk_hex',
        'public_spend_key': 'psk_hex',
        'public_view_key': 'pvk_hex',
        'success': true,
        'error': null,
      };

      final response = KeysDerivedResponse.fromJson(json);
      expect(response.address, '5addr...');
      expect(response.secretSpendKey, 'ssk_hex');
      expect(response.secretViewKey, 'svk_hex');
      expect(response.publicSpendKey, 'psk_hex');
      expect(response.publicViewKey, 'pvk_hex');
      expect(response.success, true);
    });
  });

  group('RustSignal: TransactionBroadcastResponse', () {
    test('with default booleans', () {
      // Simulate Rust omitting optional booleans (they default to false)
      final json = {
        'success': true,
        'tx_id': 'broadcasted_txid',
      };

      final response = TransactionBroadcastResponse.fromJson(json);
      expect(response.success, true);
      expect(response.txId, 'broadcasted_txid');
      expect(response.isRetryable, false);
      expect(response.isDoubleSpend, false);
    });

    test('double spend detected', () {
      final json = {
        'success': false,
        'error': 'Double spend',
        'tx_id': null,
        'is_retryable': false,
        'is_double_spend': true,
      };

      final response = TransactionBroadcastResponse.fromJson(json);
      expect(response.isDoubleSpend, true);
      expect(response.isRetryable, false);
    });
  });

  group('RustSignal: DoubleSpendDetectedResponse', () {
    test('with multiple conflicts', () {
      final json = {
        'conflicts': [
          {
            'key_image': 'ki_a',
            'previous_spent_height': 100,
            'new_height': 105,
          },
          {
            'key_image': 'ki_b',
            'previous_spent_height': 0,
            'new_height': 0,
          },
        ],
      };

      final response = DoubleSpendDetectedResponse.fromJson(json);
      expect(response.conflicts.length, 2);
      expect(response.conflicts[0].keyImage, 'ki_a');
      expect(response.conflicts[1].previousSpentHeight, 0);
      expect(response.conflicts[1].newHeight, 0);
    });
  });

  group('RustSignal: ReorgDetectedResponse', () {
    test('all fields', () {
      final json = {
        'split_height': 1000,
        'blocks_detached': 5,
        'outputs_removed': 2,
        'outputs_unspent': 1,
        'removed_key_images': ['rki1', 'rki2'],
        'unspent_key_images': ['uki1'],
      };

      final response = ReorgDetectedResponse.fromJson(json);
      expect(response.splitHeight, 1000);
      expect(response.blocksDetached, 5);
      expect(response.outputsRemoved, 2);
      expect(response.outputsUnspent, 1);
      expect(response.removedKeyImages, ['rki1', 'rki2']);
      expect(response.unspentKeyImages, ['uki1']);
    });
  });

  group('RustSignal: UnsignedTransactionCreatedResponse', () {
    test('success with recipients', () {
      final json = {
        'success': true,
        'error': null,
        'unsigned_tx_hex': 'aabbccdd',
        'fee': 50000000,
        'recipients': [
          {'address': '5dest1', 'amount': 1000000000000},
        ],
      };

      final response = UnsignedTransactionCreatedResponse.fromJson(json);
      expect(response.success, true);
      expect(response.unsignedTxHex, 'aabbccdd');
      expect(response.fee, 50000000);
      expect(response.recipients.length, 1);
      expect(response.recipients[0].address, '5dest1');
    });
  });

  group('RustSignal: TransactionSignedOfflineResponse', () {
    test('success with change outputs', () {
      final json = {
        'success': true,
        'error': null,
        'tx_id': 'signed_txid',
        'fee': 44000000,
        'tx_blob': 'signed_blob',
        'tx_key': 'signed_key',
        'tx_key_additional': [],
        'change_outputs': [
          {
            'tx_hash': 'signed_change_tx',
            'output_index': 1,
            'amount': 800000,
            'amount_xmr': '0.000000800000',
            'key': 'ck',
            'key_offset': 'co',
            'commitment_mask': 'cm',
            'received_output_bytes': 'cb',
            'key_image': 'cki',
          }
        ],
      };

      final response = TransactionSignedOfflineResponse.fromJson(json);
      expect(response.txId, 'signed_txid');
      expect(response.fee, 44000000);
      expect(response.changeOutputs.length, 1);
      expect(response.changeOutputs[0].subaddressIndex, isNull);
    });
  });

  // -----------------------------------------------------------------------
  // Edge cases
  // -----------------------------------------------------------------------

  group('Edge cases', () {
    test('zero amount and zero height', () {
      final output = OwnedOutput(
        txHash: 'zero_tx',
        outputIndex: 0,
        amount: 0,
        amountXmr: '0.000000000000',
        key: 'k',
        keyOffset: 'o',
        commitmentMask: 'm',
        receivedOutputBytes: 'b',
        blockHeight: 0,
        spent: false,
        keyImage: 'ki',
        isCoinbase: false,
        frozen: false,
      );

      final json = output.toJson();
      final restored = OwnedOutput.fromJson(json);
      expect(restored.amount, 0);
      expect(restored.blockHeight, 0);
    });

    test('empty string fields', () {
      final json = {
        'success': true,
        'error': null,
        'tx_id': '',
        'fee': 0,
        'tx_blob': '',
        'tx_key': '',
        'tx_key_additional': [],
        'spent_output_hashes': [],
        'change_outputs': [],
      };

      final response = TransactionCreatedResponse.fromJson(json);
      expect(response.txId, '');
      expect(response.txBlob, '');
    });

    test('BalanceResponse with large values', () {
      final json = {
        'confirmed': 9007199254740991, // 2^53 - 1
        'unconfirmed': 0,
        'pending_spend': 1000000000000,
      };

      final response = BalanceResponse.fromJson(json);
      expect(response.confirmed, 9007199254740991);
      expect(response.unconfirmed, 0);
      expect(response.pendingSpend, 1000000000000);
    });

    test('OwnedOutput copyWith preserves fields', () {
      final original = OwnedOutput(
        txHash: 'copy_tx',
        outputIndex: 2,
        amount: 12345,
        amountXmr: '0.000000012345',
        key: 'k',
        keyOffset: 'o',
        commitmentMask: 'm',
        subaddressIndex: (1, 7),
        paymentId: 'pid',
        receivedOutputBytes: 'b',
        blockHeight: 999,
        spent: false,
        keyImage: 'ki',
        isCoinbase: false,
        frozen: false,
      );

      final modified = original.copyWith(spent: true, frozen: true);
      expect(modified.spent, true);
      expect(modified.frozen, true);
      expect(modified.txHash, 'copy_tx');
      expect(modified.subaddressIndex, (1, 7));
      expect(modified.paymentId, 'pid');
      expect(modified.amount, 12345);
    });
  });
}
