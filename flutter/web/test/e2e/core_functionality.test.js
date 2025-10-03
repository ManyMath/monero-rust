const puppeteer = require('puppeteer');
const path = require('path');
const fs = require('fs');

/** Send `signalFnName` and resolve when `responseTypeName` is received. */
async function sendSignalAndWait(page, signalFnName, requestJson, responseTypeName, timeoutMs = 10000) {
  return page.evaluate(
    ({ signalFnName, requestJson, responseTypeName, timeoutMs }) => {
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
        window.wasmBindings[signalFnName](requestJson);
      });
    },
    { signalFnName, requestJson, responseTypeName, timeoutMs }
  );
}

/** Navigate to a fresh extension page and wait for WASM initialization. */
async function reloadExtensionPage(browser, extId) {
  const newPage = await browser.newPage();
  await newPage.goto(`chrome-extension://${extId}/index.html`);
  await newPage.waitForSelector('flt-glass-pane', { timeout: 15000 });
  await new Promise(resolve => setTimeout(resolve, 3000));
  return newPage;
}

describe('Core Monero WASM', () => {
  let browser;
  let extPage;
  let extId;

  const EXT_PATH = path.join(__dirname, '../../build/extension');

  const TEST_SEED = 'hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden';
  const EXPECTED_ADDRESS = '569ubRY6tYfgF3VpxQumrUCRaEtdyyh6NG8sVD3YRVVJbK1jkpJ3zq8WHLijVzodQ22LxwkdWx7fS2a6JzaRGzkNU654PZu';

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
        '--window-size=1280,1024'
      ],
      executablePath: process.env.CHROME_PATH || undefined,
      defaultViewport: { width: 1280, height: 1024 }
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

  it('loads extension with WASM bindings', async () => {
    const result = await extPage.evaluate(() => ({
      hasWasmBindgen: typeof wasm_bindgen !== 'undefined',
      hasWasmBindings: !!(window.wasmBindings),
      hasTestApi: !!(window.wasmBindings && window.wasmBindings.TestApi),
    }));

    expect(result.hasWasmBindings || result.hasWasmBindgen).toBe(true);
    expect(result.hasTestApi).toBe(true);
  });

  it('generates valid 25-word seed', async () => {
    const seed = await extPage.evaluate(() => {
      return window.wasmBindings.TestApi.generate_seed();
    });

    const words = seed.split(' ');
    expect(words).toHaveLength(25);
    for (const word of words) {
      expect(word).toMatch(/^[a-z]{3,10}$/);
    }
  });

  it('derives correct address from seed', async () => {
    const address = await extPage.evaluate((seed) => {
      return window.wasmBindings.TestApi.derive_address(seed, 'stagenet');
    }, TEST_SEED);

    expect(address).toBe(EXPECTED_ADDRESS);
  });

  it('derives all keys from seed', async () => {
    const keys = await extPage.evaluate((seed) => {
      return window.wasmBindings.TestApi.derive_keys(seed, 'stagenet');
    }, TEST_SEED);

    expect(keys.address).toBeTruthy();
    expect(keys.secret_spend_key).toHaveLength(64);
    expect(keys.secret_view_key).toHaveLength(64);
    expect(keys.public_spend_key).toHaveLength(64);
    expect(keys.public_view_key).toHaveLength(64);
  });
});
