const puppeteer = require('puppeteer');
const path = require('path');
const fs = require('fs');
const {
  HONKED_SEED, HONKED_ADDRESS,
  HEMLOCK_SEED, HEMLOCK_ADDRESS,
  sendSignalAndWait,
  probeNode,
} = require('./fixtures');

const NODE_URL = 'http://127.0.0.1:38081';
const KNOWN_SCAN_HEIGHT = 1384526;
const KNOWN_OUTPUT_TX = '07a561e60118c0a485b20bbfac787fd8efead96a9f422d9dff4a86f2985db7c5';

function walletConfigs() {
  return [
    { seed: HONKED_SEED, network: 'stagenet', account_lookahead: 1 },
    { seed: HEMLOCK_SEED, network: 'stagenet', account_lookahead: 1 },
  ];
}

async function runIfNodeAvailable(nodeAvailable, testName, fn) {
  if (!nodeAvailable) {
    console.log(`${testName} skipped: local stagenet node unavailable`);
    return;
  }
  await fn();
}

describe('Multi-Wallet Scan & Address Derivation', () => {
  let browser;
  let extPage;
  let extId;
  let nodeAvailable = false;
  let daemonHeight = 0;
  const EXT_PATH = path.join(__dirname, '../../build/extension');

  beforeAll(async () => {
    if (!fs.existsSync(EXT_PATH)) {
      throw new Error(`Extension not found at: ${EXT_PATH}. Run 'npm run build' first.`);
    }

    nodeAvailable = await probeNode();
    if (!nodeAvailable) {
      console.log('Local stagenet node not available -- network tests will no-op');
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
      const heightResp = await sendSignalAndWait(
        extPage,
        'send_query_daemon_height_request',
        JSON.stringify({ node_url: NODE_URL }),
        'DaemonHeightResponse',
        15000
      );
      daemonHeight = heightResp.daemon_height;
    }
  }, 120000);

  afterAll(async () => {
    if (browser) await browser.close();
  });

  describe('Multi-Wallet Scanning', () => {
    it('scans a known block for multiple wallets via ScanBlockMultiWalletRequest', async () => {
      await runIfNodeAvailable(nodeAvailable, 'SCAN-01', async () => {
        expect(daemonHeight).toBeGreaterThan(KNOWN_SCAN_HEIGHT);

        const response = await sendSignalAndWait(
          extPage,
          'send_scan_block_multi_wallet_request',
          JSON.stringify({
            node_url: NODE_URL,
            block_height: KNOWN_SCAN_HEIGHT,
            wallets: walletConfigs(),
          }),
          'MultiWalletScanResponse',
          30000
        );

        expect(response.success).toBe(true);
        expect(response.block_height).toBe(KNOWN_SCAN_HEIGHT);
        expect(response.block_hash).toMatch(/^[0-9a-f]{64}$/);
        expect(Array.isArray(response.wallet_results)).toBe(true);
        expect(response.wallet_results).toHaveLength(2);

        const byAddress = new Map(response.wallet_results.map(result => [result.address, result]));
        expect(byAddress.has(HONKED_ADDRESS)).toBe(true);
        expect(byAddress.has(HEMLOCK_ADDRESS)).toBe(true);
        expect(byAddress.get(HONKED_ADDRESS).outputs.length).toBeGreaterThan(0);

        const honkedOutput = byAddress.get(HONKED_ADDRESS).outputs[0];
        expect(honkedOutput.tx_hash).toBe(KNOWN_OUTPUT_TX);
      });
    }, 30000);

    it('starts a continuous multi-wallet scan and emits a MultiWalletScanResponse', async () => {
      await runIfNodeAvailable(nodeAvailable, 'SCAN-02', async () => {
        expect(daemonHeight).toBeGreaterThan(KNOWN_SCAN_HEIGHT);

        const response = await sendSignalAndWait(
          extPage,
          'send_start_multi_wallet_scan_request',
          JSON.stringify({
            node_url: NODE_URL,
            start_height: KNOWN_SCAN_HEIGHT,
            wallets: walletConfigs(),
          }),
          'MultiWalletScanResponse',
          60000
        );

        expect(response.success).toBe(true);
        expect(response.block_height).toBeGreaterThanOrEqual(KNOWN_SCAN_HEIGHT);
        expect(response.wallet_results).toHaveLength(2);
        expect(response.wallet_results.some(result => result.address === HONKED_ADDRESS)).toBe(true);

        await extPage.evaluate(() => {
          window.wasmBindings.send_stop_scan_request('{}');
        });
      });
    }, 70000);

    it('scans a known block for one wallet via ScanBlockRequest', async () => {
      await runIfNodeAvailable(nodeAvailable, 'SCAN-04', async () => {
        expect(daemonHeight).toBeGreaterThan(KNOWN_SCAN_HEIGHT);

        const response = await sendSignalAndWait(
          extPage,
          'send_scan_block_request',
          JSON.stringify({
            node_url: NODE_URL,
            block_height: KNOWN_SCAN_HEIGHT,
            seed: HONKED_SEED,
            network: 'stagenet',
            passphrase: '',
            bip39_account_index: 0,
          }),
          'BlockScanResponse',
          30000
        );

        expect(response.success).toBe(true);
        expect(response.block_height).toBe(KNOWN_SCAN_HEIGHT);
        expect(response.block_hash).toMatch(/^[0-9a-f]{64}$/);
        expect(response.outputs.length).toBeGreaterThan(0);
        expect(response.outputs.some(output => output.tx_hash === KNOWN_OUTPUT_TX)).toBe(true);
      });
    }, 30000);
  });

  describe('Address Derivation', () => {
    it.each([
      [HONKED_SEED, HONKED_ADDRESS],
      [HEMLOCK_SEED, HEMLOCK_ADDRESS],
    ])('derives %s via DeriveAddressRequest', async (seed, expectedAddress) => {
      const response = await sendSignalAndWait(
        extPage,
        'send_derive_address_request',
        JSON.stringify({
          seed,
          network: 'stagenet',
          passphrase: '',
          bip39_account_index: 0,
        }),
        'AddressDerivedResponse',
        10000
      );

      expect(response.success).toBe(true);
      expect(response.address).toBe(expectedAddress);
    }, 10000);
  });
});
