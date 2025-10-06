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
    // tests added in later commits
  });

  describe('Keys Export', () => {
    // tests added in later commits
  });

  describe('Subaddress Derivation', () => {
    // tests added in later commits
  });

  describe('Seed Birthday', () => {
    // tests added in later commits
  });

  const describeIfNode = nodeAvailable ? describe : describe.skip;
  describeIfNode('Network-dependent tests (future phases)', () => {
    it('placeholder: node is reachable', () => {
      expect(nodeAvailable).toBe(true);
    });
  });
});
