const puppeteer = require('puppeteer');
const path = require('path');
const fs = require('fs');
const {
  HONKED_SEED,
  HEMLOCK_ADDRESS,
  sendSignalAndWait,
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

describe('Mempool & Live-Node Transaction Tests', () => {
  let browser;
  let extPage;
  let extId;
  let nodeAvailable = false;
  let daemonHeight = 0;
  let createdTx = null;
  const EXT_PATH = path.join(__dirname, '../../build/extension');

  beforeAll(async () => {
    if (!fs.existsSync(EXT_PATH)) {
      throw new Error(`Extension not found at: ${EXT_PATH}. Run 'npm run build' first.`);
    }

    nodeAvailable = await probeNode();
    if (!nodeAvailable) {
      console.log('Local stagenet node not available -- live-node tests will no-op');
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
    if (nodeAvailable) {
      try {
        await flushDoNotRelayTransactions(NODE_URL);
      } catch (error) {
        console.warn(`Failed to flush do_not_relay tx pool cleanup: ${error.message}`);
      }
    }
  });

  it('scans the mempool via MempoolScanRequest', async () => {
    await runIfNodeAvailable(nodeAvailable, 'SCAN-03', async () => {
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
      expect(Array.isArray(response.spent_key_image_tx_hashes)).toBe(true);
    });
  }, 30000);

  it('creates a live transaction via CreateTransactionRequest', async () => {
    await runIfNodeAvailable(nodeAvailable, 'TX-01', async () => {
      expect(daemonHeight).toBeGreaterThan(LIVE_TX_SCAN_START_HEIGHT);

      const scanResp = await waitForScanCompletion(
        extPage,
        JSON.stringify({
          node_url: NODE_URL,
          start_height: LIVE_TX_SCAN_START_HEIGHT,
          seed: HONKED_SEED,
          network: 'stagenet',
          account_lookahead: 1,
          subaddress_lookahead: 0,
          passphrase: '',
          bip39_account_index: 0,
          allow_insecure_http: true,
        }),
        120000
      );

      expect(scanResp.daemon_height).toBeGreaterThan(LIVE_TX_SCAN_START_HEIGHT);
      expect(scanResp.is_scanning).toBe(false);

      const createResp = await sendSignalAndWait(
        extPage,
        'send_create_transaction_request',
        JSON.stringify({
          node_url: NODE_URL,
          seed: HONKED_SEED,
          network: 'stagenet',
          recipients: [{ address: HEMLOCK_ADDRESS, amount: 1_000_000_000 }],
          subtract_fee: false,
          passphrase: '',
          bip39_account_index: 0,
        }),
        'TransactionCreatedResponse',
        60000
      );

      expect(createResp.success).toBe(true);
      expect(createResp.tx_id).toMatch(/^[0-9a-f]{64}$/);
      expect(createResp.fee).toBeGreaterThan(0);
      expect(createResp.tx_blob).toBeTruthy();
      expect(Array.isArray(createResp.spent_output_hashes)).toBe(true);
      expect(createResp.spent_output_hashes.length).toBeGreaterThan(0);

      createdTx = createResp;
    });
  }, 130000);

  it('validates broadcast via BroadcastTransactionRequest without relaying', async () => {
    await runIfNodeAvailable(nodeAvailable, 'TX-02', async () => {
      expect(createdTx).toBeTruthy();

      const broadcastResp = await sendSignalAndWait(
        extPage,
        'send_broadcast_transaction_request',
        JSON.stringify({
          node_url: NODE_URL,
          tx_blob: createdTx.tx_blob,
          spent_output_hashes: createdTx.spent_output_hashes,
          tx_id: createdTx.tx_id,
          spent_key_images: [],
          do_not_relay: true,
        }),
        'TransactionBroadcastResponse',
        30000
      );

      expect(broadcastResp.success).toBe(true);
      expect(broadcastResp.tx_id).toBe(createdTx.tx_id);
      expect(broadcastResp.is_retryable).toBe(false);
      expect(broadcastResp.is_double_spend).toBe(false);
    });
  }, 30000);
});
