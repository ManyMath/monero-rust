const puppeteer = require('puppeteer');
const path = require('path');
const fs = require('fs');
const {
  HONKED_SEED, HONKED_ADDRESS,
  BIP39_TEST_MNEMONIC,
  sendSignalAndWait,
  reloadExtensionPage,
  probeNode,
} = require('./fixtures');

describe('Offline Signal Tests', () => {
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
  }, 120000);

  afterAll(async () => {
    if (browser) await browser.close();
  });

  // Smoke test
  it('loads extension with WASM signal bindings', async () => {
    const result = await extPage.evaluate(() => ({
      hasWasmBindings: !!(window.wasmBindings),
      hasSendRestore: typeof window.wasmBindings?.send_restore_wallet_data_request === 'function',
      hasSendBalance: typeof window.wasmBindings?.send_get_balance_request === 'function',
      hasSendExportKeys: typeof window.wasmBindings?.send_export_keys_file_request === 'function',
      hasSendDeriveSubaddress: typeof window.wasmBindings?.send_derive_subaddress_request === 'function',
    }));
    expect(result.hasWasmBindings).toBe(true);
    expect(result.hasSendRestore).toBe(true);
    expect(result.hasSendBalance).toBe(true);
    expect(result.hasSendExportKeys).toBe(true);
    expect(result.hasSendDeriveSubaddress).toBe(true);
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
      expect(importResp.spend_secret_key).toMatch(/^[0-9a-f]{64}$/);
      expect(importResp.view_secret_key).toMatch(/^[0-9a-f]{64}$/);
      expect(importResp.spend_public_key).toMatch(/^[0-9a-f]{64}$/);
      expect(importResp.view_public_key).toMatch(/^[0-9a-f]{64}$/);
    }, 20000);
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

  const describeIfNode = nodeAvailable ? describe : describe.skip;
  describeIfNode('Network-dependent tests (future phases)', () => {
    it('placeholder: node is reachable', () => {
      expect(nodeAvailable).toBe(true);
    });
  });
});
