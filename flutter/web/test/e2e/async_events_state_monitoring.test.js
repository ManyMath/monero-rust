const puppeteer = require('puppeteer');
const path = require('path');
const fs = require('fs');
const {
  HONKED_SEED,
  sendSignalAndWait,
  probeNode,
} = require('./fixtures');

const NODE_URL = 'http://127.0.0.1:38081';

// These historical blocks currently expose stable spent-key-image activity for the fixture wallet.
const SPENT_STATUS_FIXTURE = {
  blockHeight: 2043388,
  blockHash: '647452847e059c802805e3027725e82132e8952e0bb397e404420bb15491c849',
  spentKeyImage: 'd54ef9a94bdcb45830b9dbfec998902499762936e0f263543c912246e78226da',
};

const DOUBLE_SPEND_FIXTURE = {
  blockHeight: 2043397,
  spentKeyImage: 'e2f8a9ff1aa1f5cef2bc42bcca6ce31ef776149260bc846bd3d43d028615d6c0',
  spendingTxHash: '580a273d7ec34518c89a004768e007ab2cd93646e6463bcd37785a061619b300',
  previousSpentHeight: 2043300,
};

const SYNTHETIC_OUTPUT_TEMPLATE = {
  tx_hash: 'ba211db33d08d362fc57951d57e84335d6a5487057b0ff8f46b1c330ed42b702',
  output_index: 1,
  amount: 10000000000,
  amount_xmr: '0.010000000000',
  key: 'fdacdad1f204cfd5e12c5c09c5ccd3faebf07d9053a0394dc931fee51321918d',
  key_offset: 'e3546251256a028d3a4fcb44477890e0508abc19ae027c8ae2b4bdc176c0d50c',
  commitment_mask: '4f15aba286b4a774f9d623b35dc61b8ffb69252504b2c936a38e94a6dd31e10e',
  subaddress_index: [0, 2],
  payment_id: null,
  received_output_bytes:
    'ba211db33d08d362fc57951d57e84335d6a5487057b0ff8f46b1c330ed42b70201fdacdad1f204cfd5e12c5c09c5ccd3faebf07d9053a0394dc931fee51321918de3546251256a028d3a4fcb44477890e0508abc19ae027c8ae2b4bdc176c0d50c4f15aba286b4a774f9d623b35dc61b8ffb69252504b2c936a38e94a6dd31e10e00e40b5402000000010000000002000000000000000000000000000000',
  block_height: SPENT_STATUS_FIXTURE.blockHeight,
  spent: false,
  key_image: '52aca8687229ce495e1848c3a6e3666bdeb9017d926f20fd8f037dbc7c401563',
  is_coinbase: false,
  frozen: false,
};

function runIfNodeAvailable(nodeAvailable, testName, fn) {
  if (!nodeAvailable) {
    console.log(`${testName} skipped: local stagenet node unavailable`);
    return Promise.resolve();
  }
  return fn();
}

function makeSyntheticOutput(keyImage, overrides = {}) {
  return {
    ...SYNTHETIC_OUTPUT_TEMPLATE,
    key_image: keyImage,
    ...overrides,
  };
}

async function restoreWallet(page, payload) {
  await page.evaluate(({ payload }) => {
    window.wasmBindings.send_restore_wallet_data_request(JSON.stringify(payload));
  }, { payload });
  await new Promise(resolve => setTimeout(resolve, 500));
}

async function stopScan(page) {
  await page.evaluate(() => {
    window.wasmBindings.send_stop_scan_request('{}');
  });
  await new Promise(resolve => setTimeout(resolve, 250));
}

async function sendSignalAndCollect(page, signalFnName, requestJson, expectedTypes, timeoutMs = 10000) {
  await page.waitForFunction(
    () =>
      !!window.wasmBindings &&
      typeof window.wasmBindings.register_rust_signal_callback === 'function',
    { timeout: timeoutMs }
  );

  return page.evaluate(
    ({ signalFnName, requestJson, expectedTypes, timeoutMs }) => {
      return new Promise((resolve, reject) => {
        const seen = {};
        const pendingTypes = new Set(expectedTypes);
        const timeout = setTimeout(() => {
          reject(new Error(`Timeout waiting for ${[...pendingTypes].join(', ')} after ${timeoutMs}ms`));
        }, timeoutMs);

        const origCallback = window._rustSignalCallback;
        window.wasmBindings.register_rust_signal_callback((typeName, json) => {
          if (origCallback) {
            try { origCallback(typeName, json); } catch (e) { /* ignore */ }
          }

          if (!pendingTypes.has(typeName)) {
            return;
          }

          seen[typeName] = JSON.parse(json);
          pendingTypes.delete(typeName);

          if (pendingTypes.size === 0) {
            clearTimeout(timeout);
            resolve(seen);
          }
        });

        window.wasmBindings[signalFnName](requestJson);
      });
    },
    { signalFnName, requestJson, expectedTypes, timeoutMs }
  );
}

describe('Async Events & State Monitoring', () => {
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
      console.log('Local stagenet node not available -- async event tests will no-op');
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

  beforeEach(async () => {
    await restoreWallet(extPage, {
      seed: HONKED_SEED,
      network: 'stagenet',
      outputs: [],
      daemon_height: 0,
      current_height: 0,
    });
  });

  it('returns synthetic pending transaction state via GetPendingStateRequest', async () => {
    await runIfNodeAvailable(nodeAvailable, 'EVT-01', async () => {
      const createdAt = Math.floor(Date.now() / 1000);
      const pendingState = {
        pending_spends: {
          [SPENT_STATUS_FIXTURE.spentKeyImage]: {
            tx_id: 'synthetic_pending_tx',
            key_image: SPENT_STATUS_FIXTURE.spentKeyImage,
            output_key: `${SYNTHETIC_OUTPUT_TEMPLATE.tx_hash}:${SYNTHETIC_OUTPUT_TEMPLATE.output_index}`,
            amount: SYNTHETIC_OUTPUT_TEMPLATE.amount,
            created_at_secs: createdAt,
          },
        },
        tracked_transactions: {},
      };

      await restoreWallet(extPage, {
        seed: HONKED_SEED,
        network: 'stagenet',
        outputs: [makeSyntheticOutput(SPENT_STATUS_FIXTURE.spentKeyImage)],
        daemon_height: SPENT_STATUS_FIXTURE.blockHeight + 20,
        current_height: SPENT_STATUS_FIXTURE.blockHeight,
        pending_state_json: JSON.stringify(pendingState),
      });

      const response = await sendSignalAndWait(
        extPage,
        'send_get_pending_state_request',
        '{}',
        'PendingStateResponse',
        10000
      );

      expect(response.success).toBe(true);
      const restored = JSON.parse(response.pending_state_json);
      expect(restored.pending_spends[SPENT_STATUS_FIXTURE.spentKeyImage]).toMatchObject({
        tx_id: 'synthetic_pending_tx',
        key_image: SPENT_STATUS_FIXTURE.spentKeyImage,
        amount: SYNTHETIC_OUTPUT_TEMPLATE.amount,
      });
      expect(restored.tracked_transactions).toEqual({});
    });
  }, 30000);

  it('fires SpentStatusUpdatedResponse and clears the pending entry on historical replay', async () => {
    await runIfNodeAvailable(nodeAvailable, 'EVT-02', async () => {
      const createdAt = Math.floor(Date.now() / 1000);
      await restoreWallet(extPage, {
        seed: HONKED_SEED,
        network: 'stagenet',
        outputs: [makeSyntheticOutput(SPENT_STATUS_FIXTURE.spentKeyImage)],
        daemon_height: SPENT_STATUS_FIXTURE.blockHeight + 20,
        current_height: SPENT_STATUS_FIXTURE.blockHeight - 1,
        pending_state_json: JSON.stringify({
          pending_spends: {
            [SPENT_STATUS_FIXTURE.spentKeyImage]: {
              tx_id: 'synthetic_pending_tx',
              key_image: SPENT_STATUS_FIXTURE.spentKeyImage,
              output_key: `${SYNTHETIC_OUTPUT_TEMPLATE.tx_hash}:${SYNTHETIC_OUTPUT_TEMPLATE.output_index}`,
              amount: SYNTHETIC_OUTPUT_TEMPLATE.amount,
              created_at_secs: createdAt,
            },
          },
          tracked_transactions: {},
        }),
      });

      const events = await sendSignalAndCollect(
        extPage,
        'send_scan_block_request',
        JSON.stringify({
          node_url: NODE_URL,
          block_height: SPENT_STATUS_FIXTURE.blockHeight,
          seed: HONKED_SEED,
          network: 'stagenet',
          passphrase: '',
          bip39_account_index: 0,
        }),
        ['BlockScanResponse', 'SpentStatusUpdatedResponse'],
        30000
      );

      expect(events.BlockScanResponse.success).toBe(true);
      expect(events.BlockScanResponse.block_hash).toBe(SPENT_STATUS_FIXTURE.blockHash);
      expect(events.SpentStatusUpdatedResponse.spent_key_images).toContain(
        SPENT_STATUS_FIXTURE.spentKeyImage
      );

      const pendingState = await sendSignalAndWait(
        extPage,
        'send_get_pending_state_request',
        '{}',
        'PendingStateResponse',
        10000
      );
      const restored = JSON.parse(pendingState.pending_state_json);
      expect(restored.pending_spends).toEqual({});
    });
  }, 40000);

  it('fires DoubleSpendDetectedResponse when a restored confirmed spend is replayed at a new height', async () => {
    await runIfNodeAvailable(nodeAvailable, 'EVT-04', async () => {
      await restoreWallet(extPage, {
        seed: HONKED_SEED,
        network: 'stagenet',
        outputs: [
          makeSyntheticOutput(DOUBLE_SPEND_FIXTURE.spentKeyImage, {
            spent: true,
            spent_height: DOUBLE_SPEND_FIXTURE.previousSpentHeight,
            block_height: DOUBLE_SPEND_FIXTURE.previousSpentHeight - 10,
          }),
        ],
        daemon_height: DOUBLE_SPEND_FIXTURE.blockHeight + 20,
        current_height: DOUBLE_SPEND_FIXTURE.previousSpentHeight,
      });

      const events = await sendSignalAndCollect(
        extPage,
        'send_scan_block_request',
        JSON.stringify({
          node_url: NODE_URL,
          block_height: DOUBLE_SPEND_FIXTURE.blockHeight,
          seed: HONKED_SEED,
          network: 'stagenet',
          passphrase: '',
          bip39_account_index: 0,
        }),
        ['BlockScanResponse', 'DoubleSpendDetectedResponse'],
        30000
      );

      expect(events.BlockScanResponse.success).toBe(true);
      expect(events.BlockScanResponse.spent_key_images).toContain(DOUBLE_SPEND_FIXTURE.spentKeyImage);
      expect(events.BlockScanResponse.spent_key_image_tx_hashes).toContain(
        DOUBLE_SPEND_FIXTURE.spendingTxHash
      );
      expect(events.DoubleSpendDetectedResponse.conflicts).toContainEqual({
        key_image: DOUBLE_SPEND_FIXTURE.spentKeyImage,
        previous_spent_height: DOUBLE_SPEND_FIXTURE.previousSpentHeight,
        new_height: DOUBLE_SPEND_FIXTURE.blockHeight,
      });
    });
  }, 30000);

  it('fires ReorgDetectedResponse from synthetic block-hash history drift without regtest', async () => {
    await runIfNodeAvailable(nodeAvailable, 'EVT-03', async () => {
      await restoreWallet(extPage, {
        seed: HONKED_SEED,
        network: 'stagenet',
        outputs: [],
        daemon_height: SPENT_STATUS_FIXTURE.blockHeight + 20,
        current_height: SPENT_STATUS_FIXTURE.blockHeight - 1,
        block_hashes_json: JSON.stringify({
          hashes: {
            [SPENT_STATUS_FIXTURE.blockHeight]:
              'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
          },
          genesis_hash: null,
        }),
      });

      const reorgResponse = await sendSignalAndWait(
        extPage,
        'send_start_continuous_scan_request',
        JSON.stringify({
          node_url: NODE_URL,
          start_height: SPENT_STATUS_FIXTURE.blockHeight,
          seed: HONKED_SEED,
          network: 'stagenet',
          account_lookahead: 1,
          subaddress_lookahead: 0,
          passphrase: '',
          bip39_account_index: 0,
          allow_insecure_http: true,
        }),
        'ReorgDetectedResponse',
        30000
      );

      expect(reorgResponse.split_height).toBe(SPENT_STATUS_FIXTURE.blockHeight);
      expect(reorgResponse.blocks_detached).toBeGreaterThan(0);
      expect(Array.isArray(reorgResponse.removed_key_images)).toBe(true);
      expect(Array.isArray(reorgResponse.unspent_key_images)).toBe(true);

      await stopScan(extPage);
    });
  }, 40000);
});
