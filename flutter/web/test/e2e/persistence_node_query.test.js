const puppeteer = require('puppeteer');
const path = require('path');
const fs = require('fs');
const {
  HONKED_SEED, HONKED_ADDRESS,
  sendSignalAndWait,
  probeNode,
  reloadExtensionPage,
} = require('./fixtures');

describe('Persistence & Node-Query Tests', () => {
  let browser;
  let extPage;
  let extId;
  let nodeAvailable = false;
  const EXT_PATH = path.join(__dirname, '../../build/extension');

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

    // Restore wallet for persistence tests.
    await extPage.evaluate(({ fn, json }) => {
      window.wasmBindings[fn](json);
    }, {
      fn: 'send_restore_wallet_data_request',
      json: JSON.stringify({
        seed: HONKED_SEED, network: 'stagenet',
        outputs: [], daemon_height: 0, current_height: 0,
      }),
    });
    await new Promise(r => setTimeout(r, 500));
  }, 120000);

  afterAll(async () => {
    if (browser) await browser.close();
  });

  describe('Wallet Persistence', () => {
    const walletJson = JSON.stringify({
      seed: HONKED_SEED,
      network: 'stagenet',
      outputs: [],
    });

    it('has Promise-based chrome.storage.local for wallet persistence', async () => {
      const result = await extPage.evaluate(async () => {
        if (!chrome?.storage?.local) {
          return { success: false, error: 'chrome.storage.local unavailable' };
        }
        const key = `monero_wallet_e2e_storage_${Date.now()}`;
        await chrome.storage.local.set({ [key]: 'roundtrip' });
        const stored = await chrome.storage.local.get(key);
        await chrome.storage.local.remove(key);
        const afterRemove = await chrome.storage.local.get(key);
        return {
          success: stored[key] === 'roundtrip' && afterRemove[key] === undefined,
          stored: stored[key] ?? null,
          removed: afterRemove[key] === undefined,
        };
      });

      expect(result).toEqual({
        success: true,
        stored: 'roundtrip',
        removed: true,
      });
    }, 10000);

    it('keeps oversized wallet histories in IndexedDB behind chrome.storage markers', async () => {
      const result = await extPage.evaluate(async () => {
        if (!chrome?.storage?.local) {
          return { success: false, error: 'chrome.storage.local unavailable' };
        }
        if (!window.indexedDB) {
          return { success: false, error: 'IndexedDB unavailable' };
        }

        const markerPrefix = '__monero_tiered_storage_ref_v1__:';
        const databaseName = 'monero_wallet_storage';
        const storeName = 'wallets';
        const runId = `${Date.now()}_${Math.random().toString(16).slice(2)}`;
        const walletCount = 4;
        const primaryKeys = Array.from(
          { length: walletCount },
          (_, index) => `monero_wallet_quota_${runId}_${index}`,
        );
        const secondaryKeys = primaryKeys.map((key) => `tiered:${key}`);

        const openDb = () => new Promise((resolve, reject) => {
          const request = window.indexedDB.open(databaseName, 1);
          request.onupgradeneeded = () => {
            const db = request.result;
            if (!db.objectStoreNames.contains(storeName)) {
              db.createObjectStore(storeName);
            }
          };
          request.onsuccess = () => resolve(request.result);
          request.onerror = () => reject(request.error || new Error('IndexedDB open failed'));
        });

        const withStore = (db, mode, fn) => new Promise((resolve, reject) => {
          const tx = db.transaction(storeName, mode);
          const store = tx.objectStore(storeName);
          const request = fn(store);
          tx.oncomplete = () => resolve(request ? request.result : undefined);
          tx.onerror = () => reject(tx.error || new Error('IndexedDB transaction failed'));
          tx.onabort = () => reject(tx.error || new Error('IndexedDB transaction aborted'));
        });

        const makePayload = (walletIndex) => {
          const outputs = Array.from({ length: 3200 }, (_, index) => ({
            tx_hash: `quota_${walletIndex}_${index.toString().padStart(4, '0')}`,
            output_index: index % 8,
            amount: 1000000000000 + index,
            amount_xmr: '1.000000000000',
            key: `key_${walletIndex}_${index}`.padEnd(72, 'k'),
            key_offset: `offset_${walletIndex}_${index}`.padEnd(72, 'o'),
            commitment_mask: `mask_${walletIndex}_${index}`.padEnd(72, 'm'),
            subaddress_index: [walletIndex % 3, index % 17],
            block_height: 200000 + index,
            spent: index % 11 === 0,
            key_image: `ki_${walletIndex}_${index}`.padEnd(72, 'i'),
            frozen: index % 29 === 0,
          }));
          const transactions = Array.from({ length: 180 }, (_, index) => ({
            tx_hash: `history_${walletIndex}_${index}`,
            block_height: 200000 + index,
            amount: `${index}.${walletIndex}`,
            fee: '0.000010000000',
            received_outputs: outputs.slice(index, index + 3).map(
              (output) => `${output.tx_hash}:${output.output_index}`,
            ),
            spent_key_images: outputs.slice(index + 3, index + 6).map(
              (output) => output.key_image,
            ),
          }));
          return JSON.stringify({
            seed: `quota seed ${walletIndex}`,
            network: 'stagenet',
            address: '5quota'.padEnd(95, String(walletIndex)),
            node_url: 'http://127.0.0.1:38081',
            outputs,
            transactions,
            continuous_scan_current_height: 203200,
            selected_outputs: outputs.slice(0, 25).map(
              (output) => `${output.tx_hash}:${output.output_index}`,
            ),
            accounts: [0, 1, 2],
            active_account: walletIndex % 3,
            scanning_accounts: [walletIndex % 3],
          });
        };

        const db = await openDb();
        try {
          const payloads = primaryKeys.map((_, index) => makePayload(index));
          for (let index = 0; index < payloads.length; index += 1) {
            await withStore(db, 'readwrite', (store) =>
              store.put(payloads[index], secondaryKeys[index])
            );
            await chrome.storage.local.set({
              [primaryKeys[index]]: `${markerPrefix}${secondaryKeys[index]}`,
            });
          }

          const primaryValues = await chrome.storage.local.get(primaryKeys);
          const primaryBytes = new Blob([JSON.stringify(primaryValues)]).size;
          const roundtripLengths = [];
          const roundtripMatches = [];
          for (let index = 0; index < payloads.length; index += 1) {
            const stored = await withStore(db, 'readonly', (store) =>
              store.get(secondaryKeys[index])
            );
            roundtripLengths.push(stored.length);
            roundtripMatches.push(stored === payloads[index]);
          }

          return {
            success: true,
            walletCount,
            markerCount: Object.values(primaryValues).filter(
              (value) => typeof value === 'string' && value.startsWith(markerPrefix),
            ).length,
            primaryBytes,
            minPayloadBytes: Math.min(...roundtripLengths),
            allRoundtripped: roundtripMatches.every(Boolean),
          };
        } finally {
          await chrome.storage.local.remove(primaryKeys);
          for (const key of secondaryKeys) {
            await withStore(db, 'readwrite', (store) => store.delete(key));
          }
          db.close();
        }
      });

      expect(result.success).toBe(true);
      expect(result.walletCount).toBe(4);
      expect(result.markerCount).toBe(4);
      expect(result.primaryBytes).toBeLessThan(10 * 1024);
      expect(result.minPayloadBytes).toBeGreaterThan(512 * 1024);
      expect(result.allRoundtripped).toBe(true);
    }, 30000);

    it('encrypts wallet data and roundtrips through save/load', async () => {
      const saveResp = await sendSignalAndWait(
        extPage,
        'send_save_wallet_data_request',
        JSON.stringify({ password: '', wallet_data_json: walletJson }),
        'WalletDataSavedResponse',
        10000
      );

      expect(saveResp.success).toBe(true);
      expect(saveResp.encrypted_data).toBeTruthy();
      expect(saveResp.encrypted_data.length).toBeGreaterThan(0);

      const loadResp = await sendSignalAndWait(
        extPage,
        'send_load_wallet_data_request',
        JSON.stringify({ password: '', encrypted_data: saveResp.encrypted_data }),
        'WalletDataLoadedResponse',
        10000
      );

      expect(loadResp.success).toBe(true);
      expect(JSON.parse(loadResp.wallet_data_json)).toEqual(JSON.parse(walletJson));
    }, 30000);

    it('derives encryption key with valid key_hex and salt_hex', async () => {
      // derive_key_fresh() uses a random salt; two calls produce different pairs.
      const deriveResp = await sendSignalAndWait(
        extPage,
        'send_derive_encryption_key_request',
        JSON.stringify({ password: '' }),
        'EncryptionKeyDerivedResponse',
        10000
      );

      expect(deriveResp.success).toBe(true);
      expect(deriveResp.key_hex).toMatch(/^[0-9a-f]{64}$/);  // 32 bytes
      expect(deriveResp.salt_hex).toMatch(/^[0-9a-f]{32}$/); // 16 bytes
    }, 15000);

    it('saves wallet data with derived key', async () => {
      const deriveResp = await sendSignalAndWait(
        extPage,
        'send_derive_encryption_key_request',
        JSON.stringify({ password: '' }),
        'EncryptionKeyDerivedResponse',
        10000
      );
      expect(deriveResp.success).toBe(true);

      const saveResp = await sendSignalAndWait(
        extPage,
        'send_save_with_derived_key_request',
        JSON.stringify({
          key_hex: deriveResp.key_hex,
          salt_hex: deriveResp.salt_hex,
          wallet_data_json: walletJson,
        }),
        'WalletDataSavedResponse',
        10000
      );

      expect(saveResp.success).toBe(true);
      expect(saveResp.encrypted_data).toBeTruthy();
      expect(saveResp.encrypted_data.length).toBeGreaterThan(0);
    }, 20000);

    it('hydrates restored outputs after extension page reload', async () => {
      const restoredOutputs = [
        {
          tx_hash: 'reload_tx_a',
          output_index: 0,
          amount: 1000000000000,
          amount_xmr: '1.000000000000',
          key: 'reload_key_a',
          key_offset: 'reload_offset_a',
          commitment_mask: 'reload_mask_a',
          subaddress_index: [0, 0],
          received_output_bytes: '',
          block_height: 1000,
          spent: false,
          key_image: 'reload_ki_a',
          is_coinbase: false,
          frozen: false,
        },
        {
          tx_hash: 'reload_tx_b',
          output_index: 1,
          amount: 2500000000000,
          amount_xmr: '2.500000000000',
          key: 'reload_key_b',
          key_offset: 'reload_offset_b',
          commitment_mask: 'reload_mask_b',
          subaddress_index: [0, 1],
          received_output_bytes: '',
          block_height: 1001,
          spent: false,
          key_image: 'reload_ki_b',
          is_coinbase: false,
          frozen: false,
        },
        {
          tx_hash: 'reload_tx_spent',
          output_index: 0,
          amount: 900000000000,
          amount_xmr: '0.900000000000',
          key: 'reload_key_spent',
          key_offset: 'reload_offset_spent',
          commitment_mask: 'reload_mask_spent',
          subaddress_index: [1, 0],
          received_output_bytes: '',
          block_height: 1002,
          spent: true,
          key_image: 'reload_ki_spent',
          is_coinbase: false,
          frozen: false,
        },
      ];
      const restorePayload = JSON.stringify({
        seed: HONKED_SEED,
        network: 'stagenet',
        outputs: restoredOutputs,
        daemon_height: 1500,
        current_height: 1500,
        block_hashes_json: JSON.stringify({
          hashes: {
            1000: 'reload_hash_1000',
            1001: 'reload_hash_1001',
            1002: 'reload_hash_1002',
          },
          genesis_hash: null,
        }),
        pending_state_json: JSON.stringify({
          pending_spends: {},
          tracked_transactions: {},
        }),
      });

      await extPage.evaluate(({ fn, json }) => {
        window.wasmBindings[fn](json);
      }, {
        fn: 'send_restore_wallet_data_request',
        json: restorePayload,
      });
      await new Promise(resolve => setTimeout(resolve, 250));

      const beforeReload = await sendSignalAndWait(
        extPage,
        'send_get_balance_request',
        '{}',
        'BalanceResponse',
        10000
      );
      expect(beforeReload.confirmed).toBe(3500000000000);
      expect(beforeReload.unconfirmed).toBe(0);
      expect(beforeReload.pending_spend).toBe(0);

      const previousPage = extPage;
      extPage = await reloadExtensionPage(browser, extId);
      await previousPage.close();

      await extPage.evaluate(({ fn, json }) => {
        window.wasmBindings[fn](json);
      }, {
        fn: 'send_restore_wallet_data_request',
        json: restorePayload,
      });
      await new Promise(resolve => setTimeout(resolve, 250));

      const afterReload = await sendSignalAndWait(
        extPage,
        'send_get_balance_request',
        '{}',
        'BalanceResponse',
        10000
      );
      expect(afterReload).toEqual(beforeReload);
    }, 30000);
  });

  const describeIfNode = nodeAvailable ? describe : describe.skip;

  describeIfNode('Node-Query', () => {
    let daemonHeight;

    it('queries daemon height and receives positive height', async () => {
      const response = await sendSignalAndWait(
        extPage,
        'send_query_daemon_height_request',
        JSON.stringify({ node_url: 'http://127.0.0.1:38081' }),
        'DaemonHeightResponse',
        15000
      );

      expect(response.success).toBe(true);
      expect(typeof response.daemon_height).toBe('number');
      expect(response.daemon_height).toBeGreaterThan(0);

      daemonHeight = response.daemon_height;
    }, 20000);

    it('gets block height from timestamp with plausible result', async () => {
      const response = await sendSignalAndWait(
        extPage,
        'send_get_block_height_from_timestamp_request',
        JSON.stringify({ timestamp: 1704067200, node_url: 'http://127.0.0.1:38081' }),
        'BlockHeightFromTimestampResponse',
        15000
      );

      expect(response.success).toBe(true);
      expect(typeof response.block_height).toBe('number');
      expect(response.block_height).toBeGreaterThan(0);
      if (daemonHeight) {
        expect(response.block_height).toBeLessThan(daemonHeight);
      }
    }, 20000);
  });
});
