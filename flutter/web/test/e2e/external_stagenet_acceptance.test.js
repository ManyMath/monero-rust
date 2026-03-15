const puppeteer = require('puppeteer');
const path = require('path');
const fs = require('fs');
const {
  HONKED_SEED,
  sendSignalAndWait,
  waitForScanCompletion,
  probeNode,
} = require('./fixtures');

const NODE_URL = (process.env.EXTERNAL_STAGENET_NODE_URL || '').replace(/\/+$/, '');

describe('External Stagenet Acceptance', () => {
  let browser;
  let extPage;
  let extId;
  let skipReason = null;
  let daemonHeight = 0;
  const EXT_PATH = path.join(__dirname, '../../build/extension');

  beforeAll(async () => {
    if (!NODE_URL) {
      skipReason = 'EXTERNAL_STAGENET_NODE_URL is not set';
      return;
    }
    if (!fs.existsSync(EXT_PATH)) {
      throw new Error(`Extension not found at: ${EXT_PATH}. Run 'npm run build' first.`);
    }

    const nodeAvailable = await probeNode(`${NODE_URL}/get_info`, 10000);
    if (!nodeAvailable) {
      skipReason = `external stagenet node unavailable: ${NODE_URL}`;
      return;
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
    for (const target of browser.targets()) {
      const match = target.url().match(/^chrome-extension:\/\/([a-z]{32})/);
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
    await extPage.waitForFunction(
      () =>
        !!window.wasmBindings &&
        typeof window.wasmBindings.send_query_daemon_height_request === 'function',
      { timeout: 15000 }
    );
  }, 120000);

  afterAll(async () => {
    if (browser) {
      await browser.close();
    }
  });

  it('queries daemon height from a supplied external stagenet node', async () => {
    if (skipReason) {
      console.log(`EXTERNAL-STAGENET-01 skipped: ${skipReason}`);
      return;
    }

    const height = await sendSignalAndWait(
      extPage,
      'send_query_daemon_height_request',
      JSON.stringify({ node_url: NODE_URL }),
      'DaemonHeightResponse',
      15000
    );

    expect(height.daemon_height).toBeGreaterThan(0);
    daemonHeight = height.daemon_height;
  }, 30000);

  it('scans a bounded recent external stagenet range without requiring local node state', async () => {
    if (skipReason) {
      console.log(`EXTERNAL-STAGENET-02 skipped: ${skipReason}`);
      return;
    }
    if (!daemonHeight) {
      throw new Error('daemon height must be queried before scan acceptance');
    }

    const keys = await sendSignalAndWait(
      extPage,
      'send_derive_keys_request',
      JSON.stringify({ seed: HONKED_SEED, network: 'stagenet' }),
      'KeysDerivedResponse',
      10000
    );
    expect(keys.success).toBe(true);

    const scanResp = await waitForScanCompletion(
      extPage,
      JSON.stringify({
        node_url: NODE_URL,
        start_height: Math.max(0, daemonHeight - 5),
        seed: `viewonly:${keys.secret_view_key}:${keys.public_spend_key}`,
        network: 'stagenet',
        account_lookahead: 1,
        subaddress_lookahead: 0,
        passphrase: '',
        bip39_account_index: 0,
        allow_insecure_http: true,
      }),
      120000
    );

    expect(scanResp.is_scanning).toBe(false);
    expect(scanResp.daemon_height).toBeGreaterThanOrEqual(daemonHeight);
  }, 140000);

  it('scans external stagenet mempool read-only', async () => {
    if (skipReason) {
      console.log(`EXTERNAL-STAGENET-03 skipped: ${skipReason}`);
      return;
    }

    const response = await sendSignalAndWait(
      extPage,
      'send_mempool_scan_request',
      JSON.stringify({
        node_url: NODE_URL,
        seed: HONKED_SEED,
        network: 'stagenet',
        account_lookahead: 1,
        subaddress_lookahead: 0,
        passphrase: '',
        bip39_account_index: 0,
      }),
      'MempoolScanResponse',
      30000
    );

    expect(response.success).toBe(true);
    expect(typeof response.tx_count).toBe('number');
    expect(Array.isArray(response.outputs)).toBe(true);
    expect(Array.isArray(response.spent_key_images)).toBe(true);
  }, 30000);
});
