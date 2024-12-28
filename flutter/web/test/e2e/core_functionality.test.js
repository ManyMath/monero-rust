// Core Monero Functionality - WASM E2E Tests
// Tests direct WASM calls, bypassing UI

const puppeteer = require('puppeteer');
const path = require('path');
const fs = require('fs');

describe('Core Monero Functionality - Direct WASM Tests', () => {
  let browser;
  let extPage;
  let extId;

  const EXT_PATH = path.join(__dirname, '../../build/extension');
  const BUILD_TIMEOUT = 120000;
  const TEST_TIMEOUT = 30000;

  // Test vectors from sparse_scanning_demo.rs
  const TEST_SEED = 'hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden';
  const EXPECTED_ADDRESS = '569ubRY6tYfgF3VpxQumrUCRaEtdyyh6NG8sVD3YRVVJbK1jkpJ3zq8WHLijVzodQ22LxwkdWx7fS2a6JzaRGzkNU654PZu';

  beforeAll(async () => {
    if (!fs.existsSync(EXT_PATH)) {
      throw new Error(`Extension not found at: ${EXT_PATH}. Run 'npm run build' first.`);
    }

    console.log('Launching Chrome with extension...');

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
      if (url.startsWith('chrome-extension://')) {
        const match = url.match(/chrome-extension:\/\/([a-z]{32})/);
        if (match) {
          extId = match[1];
          break;
        }
      }
    }

    if (!extId) {
      throw new Error('Could not determine extension ID');
    }

    console.log(`Extension loaded with ID: ${extId}`);

    const popupUrl = `chrome-extension://${extId}/index.html`;
    extPage = await browser.newPage();

    extPage.on('console', msg => {
      const type = msg.type();
      if (type === 'error' || type === 'warn') {
        console.log(`[Browser ${type}]:`, msg.text());
      }
    });

    await extPage.goto(popupUrl);
    await extPage.waitForSelector('flt-glass-pane', { timeout: 10000 });

    // Wait for full init
    await new Promise(resolve => setTimeout(resolve, 3000));

    console.log('Extension ready for testing');

  }, BUILD_TIMEOUT);

  afterAll(async () => {
    if (browser) {
      await browser.close();
    }
  });

  it('Has TestApi WASM module available', async () => {
    console.log('\n=== Test 1: Check TestApi Module ===');

    const hasTestApi = await extPage.evaluate(() => {
      // Check various ways WASM might be exposed
      const checks = {
        hasWasmBindgen: typeof wasm_bindgen !== 'undefined',
        hasTestApi: typeof wasm_bindgen !== 'undefined' && typeof wasm_bindgen.TestApi !== 'undefined',
        hasWindow: typeof window !== 'undefined',
        windowKeys: typeof window !== 'undefined' ? Object.keys(window).filter(k => k.includes('wasm') || k.includes('Test') || k.includes('hub')).slice(0, 20) : [],
        hasModuleExports: typeof module !== 'undefined' && typeof module.exports !== 'undefined',
      };

      // Try to find the WASM module in different locations
      if (window) {
        // Check if there's a module loaded via rinf
        const rinfKeys = Object.keys(window).filter(k => k.startsWith('rinf') || k.includes('hub'));
        checks.rinfKeys = rinfKeys;

        // Check wasmBindings
        if (window.wasmBindings) {
          checks.hasWasmBindings = true;
          checks.wasmBindingsKeys = Object.keys(window.wasmBindings).slice(0, 30);
          checks.hasTestApiInWasmBindings = typeof window.wasmBindings.TestApi !== 'undefined';
        }

        // Check rinfBindings
        if (window.rinfBindings) {
          checks.hasRinfBindings = true;
          checks.rinfBindingsKeys = Object.keys(window.rinfBindings).slice(0, 30);
        }

        // Check if WASM is loaded as ES6 module
        if (window.hub || window.Hub) {
          checks.hasHubModule = true;
          checks.hubKeys = Object.keys(window.hub || window.Hub).slice(0, 20);
        }
      }

      return checks;
    });

    console.log('  WASM bindgen available:', hasTestApi.hasWasmBindgen);
    console.log('  TestApi available:', hasTestApi.hasTestApi);
    console.log('  Window keys with wasm/test/hub:', hasTestApi.windowKeys);
    console.log('  Rinf keys:', hasTestApi.rinfKeys);

    if (hasTestApi.hasWasmBindings) {
      console.log('  ✓ wasmBindings found!');
      console.log('  wasmBindings keys:', hasTestApi.wasmBindingsKeys);
      console.log('  TestApi in wasmBindings:', hasTestApi.hasTestApiInWasmBindings);
    }

    if (hasTestApi.hasRinfBindings) {
      console.log('  rinfBindings keys:', hasTestApi.rinfBindingsKeys);
    }

    if (hasTestApi.hasHubModule) {
      console.log('  Hub module keys:', hasTestApi.hubKeys);
    }

    // Check that we found the WASM bindings
    expect(hasTestApi.hasWasmBindings || hasTestApi.hasWasmBindgen).toBe(true);

  }, TEST_TIMEOUT);

  it('Generates valid 25-word seed', async () => {
    console.log('\n=== Test 2: Generate Seed ===');

    const result = await extPage.evaluate(() => {
      try {
        // Call TestApi.generate_seed() from wasmBindings
        if (window.wasmBindings && window.wasmBindings.TestApi) {
          const seed = window.wasmBindings.TestApi.generate_seed();
          const words = seed.split(' ');

          return {
            success: true,
            seed: seed,
            wordCount: words.length,
            firstWord: words[0],
            lastWord: words[words.length - 1]
          };
        } else {
          return {
            success: false,
            error: 'TestApi not available in wasmBindings'
          };
        }
      } catch (error) {
        return {
          success: false,
          error: error.message,
          stack: error.stack
        };
      }
    });

    if (result.success) {
      console.log('  ✓ Seed generated');
      console.log('  ✓ Word count:', result.wordCount);
      console.log('  ✓ First word:', result.firstWord);
      console.log('  ✓ Last word:', result.lastWord);

      expect(result.wordCount).toBe(25);
      expect(result.firstWord).toMatch(/^[a-z]{3,10}$/);
      expect(result.lastWord).toMatch(/^[a-z]{3,10}$/);
    } else {
      console.log('  ! Failed:', result.error);
      if (result.stack) {
        console.log('  Stack:', result.stack);
      }
      // Fail the test
      throw new Error(result.error);
    }

  }, TEST_TIMEOUT);

  it('Derives correct address from seed', async () => {
    console.log('\n=== Test 3: Derive Address ===');

    const result = await extPage.evaluate((testSeed, expectedAddr) => {
      try {
        if (window.wasmBindings && window.wasmBindings.TestApi) {
          const address = window.wasmBindings.TestApi.derive_address(testSeed, 'stagenet');

          return {
            success: true,
            address: address,
            matches: address === expectedAddr
          };
        } else {
          return {
            success: false,
            error: 'TestApi not available in wasmBindings'
          };
        }
      } catch (error) {
        return {
          success: false,
          error: error.message,
          stack: error.stack
        };
      }
    }, TEST_SEED, EXPECTED_ADDRESS);

    if (result.success) {
      console.log('  ✓ Address derived:', result.address.substring(0, 30) + '...');
      console.log('  ✓ Matches expected:', result.matches);

      expect(result.address).toBe(EXPECTED_ADDRESS);
    } else {
      console.log('  ! Failed:', result.error);
      if (result.stack) {
        console.log('  Stack:', result.stack);
      }
      throw new Error(result.error);
    }

  }, TEST_TIMEOUT);

  it('Derives all keys from seed', async () => {
    console.log('\n=== Test 4: Derive Keys ===');

    const result = await extPage.evaluate((testSeed) => {
      try {
        if (window.wasmBindings && window.wasmBindings.TestApi) {
          const keys = window.wasmBindings.TestApi.derive_keys(testSeed, 'stagenet');

          return {
            success: true,
            keys: keys,
            hasAddress: keys && keys.address && keys.address.length > 0,
            hasSecretSpend: keys && keys.secret_spend_key && keys.secret_spend_key.length === 64,
            hasSecretView: keys && keys.secret_view_key && keys.secret_view_key.length === 64,
            hasPublicSpend: keys && keys.public_spend_key && keys.public_spend_key.length === 64,
            hasPublicView: keys && keys.public_view_key && keys.public_view_key.length === 64
          };
        } else {
          return {
            success: false,
            error: 'TestApi not available in wasmBindings'
          };
        }
      } catch (error) {
        return {
          success: false,
          error: error.message,
          stack: error.stack
        };
      }
    }, TEST_SEED);

    if (result.success) {
      console.log('  ✓ Keys derived');
      console.log('  ✓ Has address:', result.hasAddress);
      console.log('  ✓ Secret spend key (64 hex chars):', result.hasSecretSpend);
      console.log('  ✓ Secret view key (64 hex chars):', result.hasSecretView);
      console.log('  ✓ Public spend key (64 hex chars):', result.hasPublicSpend);
      console.log('  ✓ Public view key (64 hex chars):', result.hasPublicView);

      expect(result.hasAddress).toBe(true);
      expect(result.hasSecretSpend).toBe(true);
      expect(result.hasSecretView).toBe(true);
      expect(result.hasPublicSpend).toBe(true);
      expect(result.hasPublicView).toBe(true);
    } else {
      console.log('  ! Failed:', result.error);
      if (result.stack) {
        console.log('  Stack:', result.stack);
      }
      throw new Error(result.error);
    }

  }, TEST_TIMEOUT);
});
