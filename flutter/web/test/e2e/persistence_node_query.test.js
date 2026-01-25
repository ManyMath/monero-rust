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
