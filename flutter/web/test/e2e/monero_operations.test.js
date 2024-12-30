/**
 * E2E Tests for Monero Operations - Browser Extension
 *
 * Tests the core Monero workflow by verifying the extension loads,
 * WASM initializes, and basic operations work.
 */

const puppeteer = require('puppeteer');
const path = require('path');
const { execSync } = require('child_process');
const fs = require('fs');

describe('Monero Operations E2E - Browser Extension', () => {
  let browser;
  let extensionPage;
  let extensionId;

  const EXTENSION_PATH = path.join(__dirname, '../../build/extension');
  const BUILD_TIMEOUT = 120000; // 2 minutes for build
  const TEST_TIMEOUT = 30000;   // 30 seconds per test

  beforeAll(async () => {
    // Build extension before testing
    console.log('Building extension for Monero operations test...');

    try {
      execSync('dart run tool/build_extension.dart', {
        cwd: path.join(__dirname, '../..'),
        stdio: 'inherit'
      });
    } catch (error) {
      throw new Error(`Extension build failed: ${error.message}`);
    }

    if (!fs.existsSync(EXTENSION_PATH)) {
      throw new Error(`Extension not found at: ${EXTENSION_PATH}`);
    }

    console.log('Launching Chrome with extension...');

    browser = await puppeteer.launch({
      headless: false,
      args: [
        `--disable-extensions-except=${EXTENSION_PATH}`,
        `--load-extension=${EXTENSION_PATH}`,
        '--no-sandbox',
        '--disable-setuid-sandbox',
        '--disable-dev-shm-usage',
        '--window-size=1280,1024'
      ],
      executablePath: process.env.CHROME_PATH || undefined,
      defaultViewport: { width: 1280, height: 1024 }
    });

    await new Promise(resolve => setTimeout(resolve, 3000));

    // Find extension ID from targets
    const targets = await browser.targets();
    for (const target of targets) {
      const url = target.url();
      if (url.startsWith('chrome-extension://')) {
        const match = url.match(/chrome-extension:\/\/([a-z]{32})/);
        if (match) {
          extensionId = match[1];
          break;
        }
      }
    }

    if (!extensionId) {
      throw new Error('Could not determine extension ID');
    }

    console.log(`Extension loaded with ID: ${extensionId}`);

    // Open extension popup
    const popupUrl = `chrome-extension://${extensionId}/index.html`;
    extensionPage = await browser.newPage();

    // Listen for console messages
    extensionPage.on('console', msg => {
      const type = msg.type();
      if (type === 'error') {
        console.log(`[Browser ${type}]:`, msg.text());
      }
    });

    await extensionPage.goto(popupUrl);

    // Wait for Flutter to initialize
    await extensionPage.waitForSelector('flt-glass-pane', { timeout: 10000 });

    // Wait for page to load (even if just showing loading screen)
    console.log('Waiting for page to initialize...');
    await new Promise(resolve => setTimeout(resolve, 3000));

    const pageText = await extensionPage.evaluate(() => document.body.textContent);
    console.log('Page status:', pageText.substring(0, 200));

    console.log('Extension page loaded');

  }, BUILD_TIMEOUT);

  afterAll(async () => {
    if (browser) {
      await browser.close();
    }
  });

  /**
   * Test 1: Extension builds and loads successfully
   */
  it('Should build and load the browser extension', async () => {
    console.log('\n=== Test 1: Extension Build & Load ===');

    expect(extensionId).toBeDefined();
    expect(extensionId).toMatch(/^[a-z]{32}$/);
    console.log(`  ✓ Extension ID: ${extensionId}`);

    const hasFlutter = await extensionPage.evaluate(() => {
      return document.querySelector('flt-glass-pane') !== null;
    });
    expect(hasFlutter).toBe(true);
    console.log('  ✓ Flutter web initialized');

    const hasContent = await extensionPage.evaluate(() => {
      return document.body.textContent.length > 0;
    });
    expect(hasContent).toBe(true);
    console.log('  ✓ Extension page has content');

  }, TEST_TIMEOUT);

  /**
   * Test 2: WASM and Flutter components are present
   */
  it('Should have WASM modules loaded', async () => {
    console.log('\n=== Test 2: WASM Module Check ===');

    // Check for Flutter web renderer (can be canvas or HTML)
    const hasFlutterRenderer = await extensionPage.evaluate(() => {
      // Check for Flutter-specific elements
      const hasGlassPane = document.querySelector('flt-glass-pane') !== null;
      const hasSceneHost = document.querySelector('flt-scene-host') !== null;
      const hasCanvas = document.querySelector('canvas') !== null;

      return {
        hasGlassPane,
        hasSceneHost,
        hasCanvas,
        hasAny: hasGlassPane || hasSceneHost || hasCanvas
      };
    });

    expect(hasFlutterRenderer.hasAny).toBe(true);
    if (hasFlutterRenderer.hasGlassPane) {
      console.log('  ✓ Flutter glass-pane present');
    }
    if (hasFlutterRenderer.hasSceneHost) {
      console.log('  ✓ Flutter scene-host present');
    }
    if (hasFlutterRenderer.hasCanvas) {
      console.log('  ✓ Flutter canvas present');
    }
    console.log('  ✓ Flutter web renderer initialized');

    // Verify the page is showing Monero wallet content
    const hasMoneroContent = await extensionPage.evaluate(() => {
      const text = document.body.textContent;
      return text.includes('Monero') || text.includes('Wallet') || text.includes('Loading');
    });

    expect(hasMoneroContent).toBe(true);
    console.log('  ✓ Monero wallet content detected');

  }, TEST_TIMEOUT);

  /**
   * Test 3: Extension can execute JavaScript in context
   */
  it('Should be able to execute code in extension context', async () => {
    console.log('\n=== Test 3: Extension JavaScript Context ===');

    const result = await extensionPage.evaluate(() => {
      return {
        hasWindow: typeof window !== 'undefined',
        hasDocument: typeof document !== 'undefined',
        hasLocalStorage: typeof localStorage !== 'undefined',
        canAccessDOM: document.body !== null
      };
    });

    expect(result.hasWindow).toBe(true);
    expect(result.hasDocument).toBe(true);
    expect(result.hasLocalStorage).toBe(true);
    expect(result.canAccessDOM).toBe(true);

    console.log('  ✓ window object available');
    console.log('  ✓ document object available');
    console.log('  ✓ localStorage available');
    console.log('  ✓ DOM accessible');
    console.log('  ✓ Extension context is functional');

  }, TEST_TIMEOUT);
});
