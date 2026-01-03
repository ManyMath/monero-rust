const puppeteer = require('puppeteer');
const path = require('path');
const fs = require('fs');
const {
  HONKED_SEED, HONKED_ADDRESS,
  HEMLOCK_ADDRESS,
  BIP39_TEST_MNEMONIC,
  sendSignalAndWait,
  reloadExtensionPage,
  probeNode,
  flushDoNotRelayTransactions,
  waitForScanCompletion,
} = require('./fixtures');

const NODE_URL = 'http://127.0.0.1:38081';
const LIVE_TX_SCAN_START_HEIGHT = 2043388;

async function runIfNodeAvailable(nodeAvailable, testName, fn) {
  if (!nodeAvailable) {
    console.log(`${testName} skipped: local stagenet node unavailable`);
    return;
  }
  await fn();
}

describe('Offline Signal Tests', () => {
  let browser;
  let extPage;
  let extId;
  let nodeAvailable = false;
  const EXT_PATH = path.join(__dirname, '../../build/extension');
  const COLD_SIGNING_VECTOR_PATH = path.join(
    __dirname,
    '../../../../rust/monero-rust/tests/vectors/cold_signing_regtest_v0_18_5_0'
  );
  const COLD_SIGNING_VIEW_KEY =
    '49774391fa5e8d249fc2c5b45dadef13534bf2483dede880dac88f061e809100';

  beforeAll(async () => {
    if (!fs.existsSync(EXT_PATH)) {
      throw new Error(`Extension not found at: ${EXT_PATH}. Run 'npm run build' first.`);
    }

    nodeAvailable = await probeNode();
    if (!nodeAvailable) {
      console.log('Local stagenet node not available -- network tests will be skipped');
    }

    browser = await puppeteer.launch({
      headless: false,
      args: [
        `--disable-extensions-except=${EXT_PATH}`,
        `--load-extension=${EXT_PATH}`,
        '--no-sandbox',
        '--disable-setuid-sandbox',
        '--disable-dev-shm-usage',
        '--window-size=1280,1024',
      ],
      executablePath: process.env.CHROME_PATH || undefined,
      defaultViewport: { width: 1280, height: 1024 },
    });

    await new Promise(resolve => setTimeout(resolve, 3000));

    const targets = await browser.targets();
    for (const target of targets) {
      const url = target.url();
      const match = url.match(/^chrome-extension:\/\/([a-z]{32})/);
      if (match) {
        extId = match[1];
        break;
      }
    }

    if (!extId) {
      throw new Error('Could not determine extension ID');
    }

    extPage = await browser.newPage();
    await extPage.goto(`chrome-extension://${extId}/index.html`);
    await extPage.waitForSelector('flt-glass-pane', { timeout: 10000 });
    await new Promise(resolve => setTimeout(resolve, 3000));

    if (nodeAvailable) {
      await flushDoNotRelayTransactions(NODE_URL);
    }
  }, 120000);

  afterAll(async () => {
    if (browser) await browser.close();
    if (nodeAvailable) {
      try {
        await flushDoNotRelayTransactions(NODE_URL);
      } catch (error) {
        console.warn(`Failed to flush do_not_relay tx pool cleanup: ${error.message}`);
      }
    }
  });

  // Smoke test
  it('loads extension with WASM signal bindings', async () => {
    const result = await extPage.evaluate(() => ({
      hasWasmBindings: !!(window.wasmBindings),
      hasSendRestore: typeof window.wasmBindings?.send_restore_wallet_data_request === 'function',
      hasSendBalance: typeof window.wasmBindings?.send_get_balance_request === 'function',
      hasSendExportKeys: typeof window.wasmBindings?.send_export_keys_file_request === 'function',
      hasSendDeriveSubaddress: typeof window.wasmBindings?.send_derive_subaddress_request === 'function',
      hasSendExtractSignedTxSet: typeof window.wasmBindings?.send_extract_signed_txset_request === 'function',
      hasSendInspectUnsignedTxSet: typeof window.wasmBindings?.send_inspect_unsigned_txset_request === 'function',
    }));
    expect(result.hasWasmBindings).toBe(true);
    expect(result.hasSendRestore).toBe(true);
    expect(result.hasSendBalance).toBe(true);
    expect(result.hasSendExportKeys).toBe(true);
    expect(result.hasSendDeriveSubaddress).toBe(true);
    expect(result.hasSendExtractSignedTxSet).toBe(true);
    expect(result.hasSendInspectUnsignedTxSet).toBe(true);
  }, 15000);

  describe('Wallet Restoration', () => {
    it('restores wallet from seed and reports zero balance', async () => {
      await extPage.evaluate(({ fn, json }) => {
        window.wasmBindings[fn](json);
      }, {
        fn: 'send_restore_wallet_data_request',
        json: JSON.stringify({
          seed: HONKED_SEED,
          network: 'stagenet',
          outputs: [],
          daemon_height: 0,
          current_height: 0,
        }),
      });

      await new Promise(r => setTimeout(r, 500));

      const balance = await sendSignalAndWait(
        extPage, 'send_get_balance_request', '{}', 'BalanceResponse', 10000
      );

      expect(balance).toBeDefined();
      expect(balance.confirmed).toBe(0);
      expect(balance.unconfirmed).toBe(0);
    }, 30000);

    it('converts BIP39 mnemonic to 25-word legacy seed', async () => {
      const response = await sendSignalAndWait(
        extPage,
        'send_convert_bip39_to_legacy_request',
        JSON.stringify({ bip39_mnemonic: BIP39_TEST_MNEMONIC, account_index: 0 }),
        'Bip39LegacySeedResponse',
        10000
      );

      expect(response.success).toBe(true);
      expect(response.legacy_seed.split(' ')).toHaveLength(25);
      for (const word of response.legacy_seed.split(' ')) {
        expect(word).toMatch(/^[a-z]+$/);
      }
    }, 15000);

    it('imports keys file and returns all four key hex fields', async () => {
      const exportResp = await sendSignalAndWait(
        extPage,
        'send_export_keys_file_request',
        JSON.stringify({ seed: HONKED_SEED, network: 'stagenet', password: '' }),
        'ExportKeysFileResponse',
        10000
      );
      expect(exportResp.success).toBe(true);
      expect(exportResp.file_bytes_hex).toBeTruthy();

      const importResp = await sendSignalAndWait(
        extPage,
        'send_import_keys_file_request',
        JSON.stringify({ file_bytes_hex: exportResp.file_bytes_hex, password: '' }),
        'ImportKeysFileResponse',
        10000
      );

      expect(importResp.success).toBe(true);
      expect(importResp.watch_only).toBe(false);
      expect(importResp.network).toBe('stagenet');
      expect(importResp.spend_secret_key).toMatch(/^[0-9a-f]{64}$/);
      expect(importResp.view_secret_key).toMatch(/^[0-9a-f]{64}$/);
      expect(importResp.spend_public_key).toMatch(/^[0-9a-f]{64}$/);
      expect(importResp.view_public_key).toMatch(/^[0-9a-f]{64}$/);
    }, 20000);

    it('imports generated watch-only .keys fixture with encoded network', async () => {
      const fileBytesHex = fs
        .readFileSync(path.join(COLD_SIGNING_VECTOR_PATH, 'hot_view_only.keys'))
        .toString('hex');

      const importResp = await sendSignalAndWait(
        extPage,
        'send_import_keys_file_request',
        JSON.stringify({ file_bytes_hex: fileBytesHex, password: '' }),
        'ImportKeysFileResponse',
        10000
      );

      expect(importResp.success).toBe(true);
      expect(importResp.watch_only).toBe(true);
      expect(importResp.network).toBe('mainnet');
      expect(importResp.spend_secret_key).toBe('0'.repeat(64));
      expect(importResp.view_secret_key).toBe(COLD_SIGNING_VIEW_KEY);
      expect(importResp.spend_public_key).toMatch(/^[0-9a-f]{64}$/);
      expect(importResp.view_public_key).toMatch(/^[0-9a-f]{64}$/);
      expect(importResp.mnemonic).toBeNull();
    }, 20000);
  });

  describe('Wallet2 Signed Txset Import', () => {
    it('extracts broadcastable tx blob from generated signed_monero_tx fixture', async () => {
      const signedTxSetHex = fs
        .readFileSync(path.join(COLD_SIGNING_VECTOR_PATH, 'signed_monero_tx'))
        .toString('hex');
      const metadata = JSON.parse(
        fs.readFileSync(path.join(COLD_SIGNING_VECTOR_PATH, 'metadata.json'), 'utf8')
      );
      const transferDescription = JSON.parse(
        fs.readFileSync(path.join(COLD_SIGNING_VECTOR_PATH, 'transfer_description.json'), 'utf8')
      );
      const transfer = transferDescription.desc[0];
      const keyImagesRpc = JSON.parse(
        fs.readFileSync(path.join(COLD_SIGNING_VECTOR_PATH, 'key_images_rpc.json'), 'utf8')
      );
      const expectedTxId = metadata.flow.submitted_tx_hash_list[0];

      const response = await sendSignalAndWait(
        extPage,
        'send_extract_signed_txset_request',
        JSON.stringify({
          data_hex: signedTxSetHex,
          view_key_hex: COLD_SIGNING_VIEW_KEY,
        }),
        'SignedTxSetExtractedResponse',
        10000
      );

      expect(response.success).toBe(true);
      expect(response.transactions).toHaveLength(1);
      const tx = response.transactions[0];
      expect(tx.tx_id).toBe(expectedTxId);
      expect(tx.tx_blob).toMatch(/^[0-9a-f]+$/);
      expect(tx.tx_blob.length).toBeGreaterThan(1000);
      expect(tx.tx_version).toBe(2);
      expect(tx.tx_unlock_time).toBe(transfer.unlock_time);
      expect(tx.tx_input_count).toBe(2);
      expect(tx.tx_input_ring_sizes).toEqual([transfer.ring_size, transfer.ring_size]);
      expect(tx.tx_output_count).toBe(2);
      expect(tx.tx_extra_len).toBe(transfer.extra.length / 2);
      expect(tx.rct_type).toBe(6);
      expect(tx.rct_fee).toBe(transferDescription.summary.fee);
      expect(tx.dust).toBe(0);
      expect(tx.fee).toBe(transferDescription.summary.fee);
      expect(tx.dust_added_to_fee).toBe(false);
      expect(tx.change_amount).toBe(transferDescription.summary.change_amount);
      expect(tx.selected_transfer_count).toBe(2);
      expect(tx.selected_transfer_indices).toEqual([0, 20]);
      expect(tx.key_images_len).toBeGreaterThan(0);
      expect(tx.key_images_blob_hex).toMatch(/^[0-9a-f]+$/);
      expect(tx.key_images_blob_hex.length).toBe(tx.key_images_len * 2);
      expect(tx.tx_key_is_zero).toBe(false);
      expect(tx.tx_key).toMatch(/^[0-9a-f]{64}$/);
      expect(tx.additional_tx_key_count).toBe(0);
      expect(tx.tx_key_additional).toEqual([]);
      expect(tx.destination_count).toBe(1);
      expect(tx.destination_total_amount).toBe(transfer.recipients[0].amount);
      expect(tx.multisig_sig_count).toBe(0);
      expect(response.key_images).toHaveLength(keyImagesRpc.signed_key_images.length);
      expect(response.key_images[0]).toBe(keyImagesRpc.signed_key_images[0].key_image);
      expect(response.key_images.at(-1)).toBe(keyImagesRpc.signed_key_images.at(-1).key_image);
      expect(response.tx_key_images).toHaveLength(2);
      for (const entry of response.tx_key_images) {
        expect(entry.public_key).toMatch(/^[0-9a-f]{64}$/);
        expect(entry.key_image).toMatch(/^[0-9a-f]{64}$/);
      }
    }, 20000);
  });

  describe('Wallet2 Unsigned Txset Inspection', () => {
    it('inspects generated unsigned_monero_tx construction metadata', async () => {
      const unsignedTxSetHex = fs
        .readFileSync(path.join(COLD_SIGNING_VECTOR_PATH, 'unsigned_monero_tx'))
        .toString('hex');
      const transferDescription = JSON.parse(
        fs.readFileSync(path.join(COLD_SIGNING_VECTOR_PATH, 'transfer_description.json'), 'utf8')
      );
      const transfer = transferDescription.desc[0];

      const response = await sendSignalAndWait(
        extPage,
        'send_inspect_unsigned_txset_request',
        JSON.stringify({
          data_hex: unsignedTxSetHex,
          view_key_hex: COLD_SIGNING_VIEW_KEY,
        }),
        'UnsignedTxSetInspectedResponse',
        10000
      );

      expect(response.success).toBe(true);
      expect(response.archive_version).toBe(2);
      expect(response.transaction_count).toBe(1);
      expect(response.new_transfer_first).toBe(80);
      expect(response.new_transfer_second).toBe(80);
      expect(response.new_transfer_count).toBe(0);
      expect(response.new_transfers).toEqual([]);

      const construction = response.constructions[0];
      expect(construction.source_count).toBe(2);
      expect(construction.source_ring_sizes).toEqual([16, 16]);
      expect(construction.selected_transfer_indices).toEqual([0, 20]);
      expect(construction.extra_hex).toBe(transfer.extra);
      expect(construction.unlock_time).toBe(0);
      expect(construction.construction_flags).toBe(3);
      expect(construction.use_rct).toBe(true);
      expect(construction.use_view_tags).toBe(true);
      expect(construction.rct_range_proof_type).toBe(3);
      expect(construction.rct_bp_version).toBe(4);
      expect(construction.destination_count).toBe(1);
      expect(construction.destination_total_amount).toBe(transfer.recipients[0].amount);
      expect(construction.change_amount).toBe(transfer.change_amount);
      expect(construction.change.amount).toBe(transfer.change_amount);
      expect(construction.change.original_address_hex).toBe('');
      expect(construction.change.is_subaddress).toBe(false);
      expect(construction.change.is_integrated).toBe(false);
      expect(construction.split_destination_count).toBe(2);
      expect(construction.split_destination_total_amount).toBe(
        transfer.change_amount + transfer.recipients[0].amount
      );
      expect(construction.split_destinations).toHaveLength(2);
      expect(construction.split_destinations[0].amount).toBe(transfer.change_amount);
      expect(construction.split_destinations[0].original_address_hex).toBe('');
      expect(construction.split_destinations[1].amount).toBe(transfer.recipients[0].amount);
      expect(construction.split_destinations[1].original_address_hex).toBe(
        Buffer.from(transfer.recipients[0].address, 'utf8').toString('hex')
      );

      expect(construction.sources).toHaveLength(2);
      expect(construction.sources.map(source => source.real_global_output_index)).toEqual([0, 20]);
      expect(construction.sources.map(source => source.rct)).toEqual([true, true]);
      expect(construction.sources.map(source => source.amount)).toEqual(
        transfer.sources.map(source => source.amount)
      );
      for (const source of construction.sources) {
        expect(source.ring).toHaveLength(source.ring_size);
        expect(source.real_output).toBeLessThan(source.ring.length);
        const realRingEntry = source.ring[source.real_output];
        expect(realRingEntry.global_output_index).toBe(source.real_global_output_index);
        expect(realRingEntry.output_public_key).toBe(source.real_output_public_key);
        expect(realRingEntry.output_public_key).toMatch(/^[0-9a-f]{64}$/);
        expect(realRingEntry.commitment).toMatch(/^[0-9a-f]{64}$/);
        expect(source.real_tx_public_key).toMatch(/^[0-9a-f]{64}$/);
        expect(source.real_out_additional_tx_keys).toEqual([]);
        expect(source.mask).toMatch(/^[0-9a-f]{64}$/);
      }

      expect(construction.destinations).toHaveLength(1);
      expect(construction.destinations[0].amount).toBe(transfer.recipients[0].amount);
      expect(construction.destinations[0].original_address_hex).toBe(
        Buffer.from(transfer.recipients[0].address, 'utf8').toString('hex')
      );
      expect(construction.destinations[0].is_subaddress).toBe(false);
      expect(construction.destinations[0].is_integrated).toBe(false);
      expect(construction.subaddr_account).toBe(0);
      expect(construction.subaddr_indices).toEqual([0]);
    }, 20000);

    it('signs generated unsigned_monero_tx through the app offline signer', async () => {
      const unsignedTxSetHex = fs
        .readFileSync(path.join(COLD_SIGNING_VECTOR_PATH, 'unsigned_monero_tx'))
        .toString('hex');
      const transferDescription = JSON.parse(
        fs.readFileSync(path.join(COLD_SIGNING_VECTOR_PATH, 'transfer_description.json'), 'utf8')
      );
      const keyImagesRpc = JSON.parse(
        fs.readFileSync(path.join(COLD_SIGNING_VECTOR_PATH, 'key_images_rpc.json'), 'utf8')
      );
      const coldKeysHex = fs
        .readFileSync(path.join(COLD_SIGNING_VECTOR_PATH, 'cold_full.keys'))
        .toString('hex');
      const importedCold = await sendSignalAndWait(
        extPage,
        'send_import_keys_file_request',
        JSON.stringify({ file_bytes_hex: coldKeysHex, password: '' }),
        'ImportKeysFileResponse',
        10000
      );
      expect(importedCold.success).toBe(true);
      expect(importedCold.watch_only).toBe(false);
      expect(importedCold.spend_secret_key).toMatch(/^[0-9a-f]{64}$/);
      expect(importedCold.view_secret_key).toMatch(/^[0-9a-f]{64}$/);

      const response = await sendSignalAndWait(
        extPage,
        'send_sign_unsigned_transaction_request',
        JSON.stringify({
          seed: importedCold.mnemonic ?? '',
          unsigned_tx_hex: unsignedTxSetHex,
          network: 'mainnet',
          spend_secret_key_hex: importedCold.spend_secret_key,
          view_secret_key_hex: importedCold.view_secret_key,
        }),
        'TransactionSignedOfflineResponse',
        60000
      );

      expect(response.success).toBe(true);
      expect(response.tx_id).toMatch(/^[0-9a-f]{64}$/);
      expect(response.tx_blob).toMatch(/^[0-9a-f]+$/);
      expect(response.fee).toBe(transferDescription.summary.fee);
      expect(response.spent_key_images).toHaveLength(2);
      expect(response.spent_key_images).toContain(keyImagesRpc.signed_key_images[0].key_image);
      expect(response.spent_key_images).toContain(keyImagesRpc.signed_key_images[20].key_image);
    }, 70000);

    it('rejects generated unsigned_monero_tx when the seed does not own the sources', async () => {
      const unsignedTxSetHex = fs
        .readFileSync(path.join(COLD_SIGNING_VECTOR_PATH, 'unsigned_monero_tx'))
        .toString('hex');

      const response = await sendSignalAndWait(
        extPage,
        'send_sign_unsigned_transaction_request',
        JSON.stringify({
          seed: HONKED_SEED,
          unsigned_tx_hex: unsignedTxSetHex,
          network: 'mainnet',
        }),
        'TransactionSignedOfflineResponse',
        60000
      );

      expect(response.success).toBe(false);
      expect(response.error).toContain('Failed to convert wallet2 unsigned txset');
      expect(response.tx_id).toBeNull();
      expect(response.tx_blob).toBeNull();
      expect(response.spent_key_images).toEqual([]);
    }, 70000);
  });

  describe('Keys Export', () => {
    it('exports keys file with non-empty file_bytes_hex', async () => {
      const response = await sendSignalAndWait(
        extPage,
        'send_export_keys_file_request',
        JSON.stringify({ seed: HONKED_SEED, network: 'stagenet', password: '' }),
        'ExportKeysFileResponse',
        10000
      );

      expect(response.success).toBe(true);
      expect(response.file_bytes_hex).toBeTruthy();
      expect(response.file_bytes_hex).toMatch(/^[0-9a-f]+$/i);
    }, 15000);
  });

  describe('Subaddress Derivation', () => {
    it('derives subaddress for account 0 index 1, distinct from primary address', async () => {
      const response = await sendSignalAndWait(
        extPage,
        'send_derive_subaddress_request',
        JSON.stringify({ seed: HONKED_SEED, network: 'stagenet', account: 0, address_index: 1 }),
        'SubaddressDerivedResponse',
        10000
      );

      expect(response.success).toBe(true);
      expect(response.address.length).toBe(95);
      expect(response.address).not.toBe(HONKED_ADDRESS);
      expect(response.address[0]).toBe('7'); // stagenet subaddresses start with '7'
    }, 15000);
  });

  describe('Seed Birthday', () => {
    it('retrieves non-null birthday for a polyseed', async () => {
      // Classic 25-word seeds return null for birthday; polyseed has one.
      const seedResp = await sendSignalAndWait(
        extPage,
        'send_generate_seed_request',
        JSON.stringify({ seed_type: 'polyseed' }),
        'SeedGeneratedResponse',
        10000
      );
      expect(seedResp.success).toBe(true);
      expect(seedResp.seed.split(' ')).toHaveLength(16);

      const birthdayResp = await sendSignalAndWait(
        extPage,
        'send_get_seed_birthday_request',
        JSON.stringify({ seed: seedResp.seed }),
        'SeedBirthdayResponse',
        10000
      );

      expect(birthdayResp.success).toBe(true);
      expect(birthdayResp.birthday).not.toBeNull();
      expect(birthdayResp.birthday).toBeGreaterThan(0);
    }, 20000);
  });

  describe('Network-dependent offline signing flow', () => {
    it('creates unsigned tx from view-only wallet, signs offline, and validates broadcast', async () => {
      await runIfNodeAvailable(nodeAvailable, 'OFFLINE-UAT-01', async () => {
        const keys = await sendSignalAndWait(
          extPage,
          'send_derive_keys_request',
          JSON.stringify({ seed: HONKED_SEED, network: 'stagenet' }),
          'KeysDerivedResponse',
          10000
        );
        expect(keys.success).toBe(true);
        expect(keys.secret_view_key).toMatch(/^[0-9a-f]{64}$/);
        expect(keys.public_spend_key).toMatch(/^[0-9a-f]{64}$/);

        const viewOnlySeed = `viewonly:${keys.secret_view_key}:${keys.public_spend_key}`;
        const scanResp = await waitForScanCompletion(
          extPage,
          JSON.stringify({
            node_url: NODE_URL,
            start_height: LIVE_TX_SCAN_START_HEIGHT,
            seed: viewOnlySeed,
            network: 'stagenet',
            account_lookahead: 1,
            subaddress_lookahead: 0,
            passphrase: '',
            bip39_account_index: 0,
          }),
          120000
        );
        expect(scanResp.is_scanning).toBe(false);
        expect(scanResp.daemon_height).toBeGreaterThan(LIVE_TX_SCAN_START_HEIGHT);

        const balance = await sendSignalAndWait(
          extPage,
          'send_get_balance_request',
          '{}',
          'BalanceResponse',
          10000
        );
        expect(balance.confirmed).toBeGreaterThan(1_000_000_000);

        const unsigned = await sendSignalAndWait(
          extPage,
          'send_create_unsigned_transaction_request',
          JSON.stringify({
            node_url: NODE_URL,
            view_key_hex: keys.secret_view_key,
            pub_spend_key_hex: keys.public_spend_key,
            network: 'stagenet',
            recipients: [{ address: HEMLOCK_ADDRESS, amount: 1_000_000_000 }],
          }),
          'UnsignedTransactionCreatedResponse',
          60000
        );
        expect(unsigned.success).toBe(true);
        expect(unsigned.unsigned_tx_hex).toMatch(/^[0-9a-f]+$/);
        expect(unsigned.fee).toBeGreaterThan(0);
        expect(unsigned.recipients).toEqual([
          { address: HEMLOCK_ADDRESS, amount: 1_000_000_000 },
        ]);

        const signed = await sendSignalAndWait(
          extPage,
          'send_sign_unsigned_transaction_request',
          JSON.stringify({
            seed: HONKED_SEED,
            unsigned_tx_hex: unsigned.unsigned_tx_hex,
            network: 'stagenet',
          }),
          'TransactionSignedOfflineResponse',
          60000
        );
        expect(signed.success).toBe(true);
        expect(signed.tx_id).toMatch(/^[0-9a-f]{64}$/);
        expect(signed.tx_blob).toMatch(/^[0-9a-f]+$/);
        expect(signed.fee).toBe(unsigned.fee);
        expect(signed.spent_key_images).toHaveLength(1);
        expect(signed.spent_key_images[0]).toMatch(/^[0-9a-f]{64}$/);

        const broadcast = await sendSignalAndWait(
          extPage,
          'send_broadcast_transaction_request',
          JSON.stringify({
            node_url: NODE_URL,
            tx_blob: signed.tx_blob,
            spent_output_hashes: [],
            tx_id: signed.tx_id,
            spent_key_images: signed.spent_key_images,
            do_not_relay: true,
          }),
          'TransactionBroadcastResponse',
          30000
        );
        expect(broadcast.success).toBe(true);
        expect(broadcast.tx_id).toBe(signed.tx_id);
        expect(broadcast.is_retryable).toBe(false);
        expect(broadcast.is_double_spend).toBe(false);
      });
    }, 180000);
  });
});
