const puppeteer = require('puppeteer');
const path = require('path');
const fs = require('fs');
const {
  HONKED_SEED, HONKED_ADDRESS,
  sendSignalAndWait,
  probeNode,
} = require('./fixtures');

// Fixture key image for offline export and freeze/thaw testing
const FIXTURE_KEY_IMAGE = 'abababababababababababababababababababababababababababababababababab';

// Synthetic output with valid Ed25519 basepoint as key -- enables offline key image export
const FIXTURE_OUTPUTS = [
  {
    tx_hash: 'aa'.repeat(32),
    output_index: 0,
    amount: 1000000000000,
    amount_xmr: '1.000000000000',
    key: '5866666666666666666666666666666666666666666666666666666666666666',
    key_offset: '0100000000000000000000000000000000000000000000000000000000000000',
    commitment_mask: '00'.repeat(32),
    received_output_bytes: '',
    block_height: 1386863,
    spent: false,
    key_image: FIXTURE_KEY_IMAGE,
    is_coinbase: false,
    frozen: false,
  }
];

describe('Key Images, Proofs & Freeze Tests', () => {
  let browser;
  let extPage;
  let extId;
  let nodeAvailable = false;
  let exportedHex = null;
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


    await extPage.evaluate(({ fn, json }) => {
      window.wasmBindings[fn](json);
    }, {
      fn: 'send_restore_wallet_data_request',
      json: JSON.stringify({
        seed: HONKED_SEED, network: 'stagenet',
        outputs: FIXTURE_OUTPUTS, daemon_height: 1386874, current_height: 1386874,
      }),
    });
    await new Promise(r => setTimeout(r, 500));
  }, 120000);

  afterAll(async () => {
    if (browser) await browser.close();
  });

  // Helper: describeIfNode for node-dependent tests
  const describeIfNode = nodeAvailable ? describe : describe.skip;


  describe('Key Image Export', () => {
    it('exports key images offline', async () => {
      const resp = await sendSignalAndWait(
        extPage,
        'send_export_key_images_request',
        JSON.stringify({ seed: HONKED_SEED, network: 'stagenet', passphrase: '', bip39_account_index: 0 }),
        'KeyImagesExportedResponse',
        30000
      );
      expect(resp.success).toBe(true);
      expect(resp.key_images_hex).toBeTruthy();
      expect(resp.key_images_hex.length).toBeGreaterThan(0);
      expect(resp.count).toBeGreaterThan(0);

      exportedHex = resp.key_images_hex;
    }, 30000);
  });


  describeIfNode('Key Image Import', () => {
    it('imports key images with node verification', async () => {
      expect(exportedHex).toBeTruthy();
      const resp = await sendSignalAndWait(
        extPage,
        'send_import_key_images_request',
        JSON.stringify({
          data_hex: exportedHex,
          node_url: 'http://127.0.0.1:38081',
          seed: HONKED_SEED,
          passphrase: '',
          bip39_account_index: 0,
        }),
        'KeyImagesImportedResponse',
        30000
      );
      expect(resp.success).toBe(true);
      expect(resp.imported_count).toBeGreaterThan(0);
    }, 30000);
  });


  describe('Payment Proofs', () => {
    it('generates OutProofV2 with test vectors', async () => {
      const resp = await sendSignalAndWait(
        extPage,
        'send_generate_out_proof_request',
        JSON.stringify({
          tx_id: '46d9f3eaf8d25b6a5d0847ad0beaece8b153d1b8c25ce317934ec17223025806',
          tx_key: '0200000000000000000000000000000000000000000000000000000000000000',
          recipient_address: '55LTR8KniP4LQGJSPtbYDacR7dz8RBFnsfAKMaMuwUNYX6aQbBcovzDPyrQF9KXF9tVU6Xk3K8no1BywnJX6GvZX8yJsXvt',
          message: '',
          network: 'stagenet',
        }),
        'OutProofGeneratedResponse',
        15000
      );
      expect(resp.success).toBe(true);
      expect(resp.signature).toBeTruthy();
      expect(resp.signature).toMatch(/^OutProofV2/);
    }, 15000);
  });


  describe('Freeze/Thaw', () => {
    it('freezes and thaws output', async () => {
      // Freeze
      const freezeResp = await sendSignalAndWait(
        extPage,
        'send_freeze_output_request',
        JSON.stringify({ key_image: FIXTURE_KEY_IMAGE }),
        'FreezeThawResponse',
        10000
      );
      expect(freezeResp.success).toBe(true);
      expect(freezeResp.frozen).toBe(true);

      // Thaw
      const thawResp = await sendSignalAndWait(
        extPage,
        'send_thaw_output_request',
        JSON.stringify({ key_image: FIXTURE_KEY_IMAGE }),
        'FreezeThawResponse',
        10000
      );
      expect(thawResp.success).toBe(true);
      expect(thawResp.frozen).toBe(false);
    }, 20000);
  });
});
