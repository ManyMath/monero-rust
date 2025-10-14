const puppeteer = require('puppeteer');
const path = require('path');
const fs = require('fs');
const {
  HONKED_SEED,
  sendSignalAndWait,
} = require('./fixtures');

async function waitForRawSignal(page, responseTypeName, trigger, timeoutMs = 10000) {
  await page.waitForFunction(
    () =>
      !!window.wasmBindings &&
      typeof window.wasmBindings.register_rust_signal_callback === 'function',
    { timeout: timeoutMs }
  );

  return page.evaluate(
    ({ responseTypeName, trigger, timeoutMs }) => {
      return new Promise((resolve, reject) => {
        const timeout = setTimeout(
          () => reject(new Error(`Timeout waiting for ${responseTypeName} after ${timeoutMs}ms`)),
          timeoutMs
        );
        const origCallback = window._rustSignalCallback;
        window.wasmBindings.register_rust_signal_callback((typeName, json) => {
          if (origCallback) {
            try { origCallback(typeName, json); } catch (e) { /* ignore */ }
          }
          if (typeName === responseTypeName) {
            clearTimeout(timeout);
            resolve(JSON.parse(json));
          }
        });
        const fn = window.wasmBindings[trigger.signalFnName];
        fn(trigger.payload);
      });
    },
    { responseTypeName, trigger, timeoutMs }
  );
}

describe('Error & Edge Cases', () => {
  let browser;
  let extPage;
  let extId;
  const EXT_PATH = path.join(__dirname, '../../build/extension');
  const walletJson = JSON.stringify({
    seed: HONKED_SEED,
    network: 'stagenet',
    outputs: [],
  });

  beforeAll(async () => {
    if (!fs.existsSync(EXT_PATH)) {
      throw new Error(`Extension not found at: ${EXT_PATH}. Run 'npm run build' first.`);
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

  it('returns a structured failure for an invalid seed phrase', async () => {
    const response = await sendSignalAndWait(
      extPage,
      'send_derive_address_request',
      JSON.stringify({
        seed: 'invalid seed phrase',
        network: 'stagenet',
        passphrase: '',
        bip39_account_index: 0,
      }),
      'AddressDerivedResponse',
      10000
    );

    expect(response.success).toBe(false);
    expect(response.error).toMatch(/invalid|seed|word/i);
    expect(response.error_code).toBeTruthy();
  }, 20000);

  it('returns ErrorResponse for malformed JSON input instead of throwing', async () => {
    const response = await waitForRawSignal(
      extPage,
      'ErrorResponse',
      {
        signalFnName: 'send_derive_address_request',
        payload: '{"seed":"broken"',
      },
      10000
    );

    expect(response.error).toMatch(/expected|eof|parse|json/i);
    expect(response.error_code).toBe(9000);
    expect(response.error_transient).toBe(false);
  }, 20000);

  it('returns a structured failure when the node is unreachable', async () => {
    const response = await sendSignalAndWait(
      extPage,
      'send_query_daemon_height_request',
      JSON.stringify({ node_url: 'http://127.0.0.1:1' }),
      'DaemonHeightResponse',
      15000
    );

    expect(response.success).toBe(false);
    expect(response.error).toBeTruthy();
    expect(response.error_code).toBeTruthy();
  }, 20000);

  it('returns a structured failure for wrong-password wallet decryption', async () => {
    const saveResponse = await sendSignalAndWait(
      extPage,
      'send_save_wallet_data_request',
      JSON.stringify({ password: 'correct-password', wallet_data_json: walletJson }),
      'WalletDataSavedResponse',
      10000
    );

    expect(saveResponse.success).toBe(true);
    expect(saveResponse.encrypted_data).toBeTruthy();

    const loadResponse = await sendSignalAndWait(
      extPage,
      'send_load_wallet_data_request',
      JSON.stringify({
        password: 'wrong-password',
        encrypted_data: saveResponse.encrypted_data,
      }),
      'WalletDataLoadedResponse',
      10000
    );

    expect(loadResponse.success).toBe(false);
    expect(loadResponse.error).toMatch(/decrypt|password|invalid/i);
    expect(loadResponse.error_code).toBeTruthy();
  }, 20000);

  it('returns a structured failure for corrupt encrypted wallet data', async () => {
    const response = await sendSignalAndWait(
      extPage,
      'send_load_wallet_data_request',
      JSON.stringify({
        password: 'irrelevant',
        encrypted_data: 'not-valid-base64!!!',
      }),
      'WalletDataLoadedResponse',
      10000
    );

    expect(response.success).toBe(false);
    expect(response.error).toMatch(/invalid encrypted data|base64/i);
    expect(response.error_code).toBeTruthy();
  }, 20000);
});
