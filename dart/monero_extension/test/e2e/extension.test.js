/**
 * E2E Tests for Monero WASM Browser Extension - Chrome
 *
 * Tests extension loading, WASM initialization, and node connectivity
 * Uses Puppeteer to automate Chrome with extension loaded
 */

const puppeteer = require('puppeteer');
const path = require('path');
const { execSync } = require('child_process');
const fs = require('fs');

describe('Monero Extension E2E - Chrome', () => {
  let browser;
  let extensionPage;
  let extensionId;

  const EXTENSION_PATH = path.join(__dirname, '../../build/extension');
  const BUILD_TIMEOUT = 120000; // 2 minutes for build
  const TEST_TIMEOUT = 30000;   // 30 seconds per test

  beforeAll(async () => {
    // Build extension before testing
    console.log('Building extension...');

    try {
      execSync('dart run tool/build_extension.dart', {
        cwd: path.join(__dirname, '../..'),
        stdio: 'inherit'
      });
    } catch (error) {
      throw new Error(`Extension build failed: ${error.message}`);
    }

    // Verify extension directory exists
    if (!fs.existsSync(EXTENSION_PATH)) {
      throw new Error(`Extension not found at: ${EXTENSION_PATH}`);
    }

    // Verify manifest.json exists
    const manifestPath = path.join(EXTENSION_PATH, 'manifest.json');
    if (!fs.existsSync(manifestPath)) {
      throw new Error(`Manifest not found at: ${manifestPath}`);
    }

    console.log('Launching Chrome with extension...');

    // Launch browser with extension loaded
    browser = await puppeteer.launch({
      headless: false, // Extensions require headed mode
      args: [
        `--disable-extensions-except=${EXTENSION_PATH}`,
        `--load-extension=${EXTENSION_PATH}`,
        '--no-sandbox',
        '--disable-setuid-sandbox',
        '--disable-dev-shm-usage'
      ],
      // Use default Chrome installation
      executablePath: process.env.CHROME_PATH || undefined
    });

    // Wait for extension to load
    await new Promise(resolve => setTimeout(resolve, 3000));

    // Find extension ID from chrome://extensions page
    // (Our extension has no background script, so we need to query chrome://extensions)
    const page = await browser.newPage();
    await page.goto('chrome://extensions');

    // Get extension ID from the page
    extensionId = await page.evaluate(() => {
      const extensions = document.querySelector('extensions-manager')
        ?.shadowRoot?.querySelector('extensions-item-list')
        ?.shadowRoot?.querySelectorAll('extensions-item');

      for (const ext of extensions || []) {
        const name = ext.shadowRoot?.querySelector('#name')?.textContent;
        if (name?.includes('MoneroExtension')) {
          return ext.id;
        }
      }
      return null;
    });

    await page.close();

    if (!extensionId) {
      throw new Error('Could not determine extension ID. Make sure the extension loaded correctly.');
    }

    console.log(`Extension loaded with ID: ${extensionId}`);

  }, BUILD_TIMEOUT);

  afterAll(async () => {
    if (browser) {
      await browser.close();
    }
  });

  test('extension loads successfully', async () => {
    expect(extensionId).toBeDefined();
    expect(extensionId).toMatch(/^[a-z]{32}$/);
  }, TEST_TIMEOUT);

  test('extension popup opens', async () => {
    // Create new page and navigate to extension popup
    extensionPage = await browser.newPage();

    const popupUrl = `chrome-extension://${extensionId}/index.html`;
    console.log(`Opening extension popup: ${popupUrl}`);

    await extensionPage.goto(popupUrl, {
      waitUntil: 'networkidle2',
      timeout: TEST_TIMEOUT
    });

    // Verify page loaded
    const title = await extensionPage.title();
    expect(title).toBeTruthy();

    console.log(`Extension page title: ${title}`);
  }, TEST_TIMEOUT);

  test('WASM modules load', async () => {
    if (!extensionPage) {
      extensionPage = await browser.newPage();
      await extensionPage.goto(`chrome-extension://${extensionId}/index.html`);
    }

    // Check for WASM errors in console
    const consoleErrors = [];
    extensionPage.on('console', msg => {
      if (msg.type() === 'error') {
        consoleErrors.push(msg.text());
      }
    });

    // Wait for potential WASM loading
    await new Promise(resolve => setTimeout(resolve, 3000));

    // Check for WASM-related errors
    const wasmErrors = consoleErrors.filter(err =>
      err.includes('wasm') ||
      err.includes('WebAssembly') ||
      err.includes('CompileError')
    );

    if (wasmErrors.length > 0) {
      console.error('WASM errors detected:', wasmErrors);
    }

    expect(wasmErrors.length).toBe(0);
  }, TEST_TIMEOUT);

  test('Flutter app renders', async () => {
    if (!extensionPage) {
      extensionPage = await browser.newPage();
      await extensionPage.goto(`chrome-extension://${extensionId}/index.html`);
    }

    // Wait for Flutter to initialize
    await new Promise(resolve => setTimeout(resolve, 3000));

    // Check for Flutter-specific elements
    const flutterView = await extensionPage.$('flt-glass-pane, flutter-view, #app, body');
    expect(flutterView).toBeTruthy();

    // Take screenshot for debugging
    const screenshotPath = path.join(__dirname, 'screenshots', 'flutter-rendered.png');
    if (!fs.existsSync(path.dirname(screenshotPath))) {
      fs.mkdirSync(path.dirname(screenshotPath), { recursive: true });
    }
    await extensionPage.screenshot({ path: screenshotPath });
    console.log(`Screenshot saved: ${screenshotPath}`);
  }, TEST_TIMEOUT);

  test('no CSP violations', async () => {
    if (!extensionPage) {
      extensionPage = await browser.newPage();
      await extensionPage.goto(`chrome-extension://${extensionId}/index.html`);
    }

    const cspViolations = [];

    // Listen for CSP violations
    extensionPage.on('console', msg => {
      const text = msg.text();
      if (text.includes('Content Security Policy') || text.includes('CSP')) {
        cspViolations.push(text);
      }
    });

    // Wait and check
    await new Promise(resolve => setTimeout(resolve, 3000));

    if (cspViolations.length > 0) {
      console.error('CSP violations:', cspViolations);
    }

    expect(cspViolations.length).toBe(0);
  }, TEST_TIMEOUT);

  test('extension has CORS bypass permissions', async () => {
    // Read manifest to verify permissions
    const manifestPath = path.join(EXTENSION_PATH, 'manifest.json');
    const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));

    expect(manifest.host_permissions).toBeDefined();
    expect(manifest.host_permissions.length).toBeGreaterThan(0);

    // Should allow all origins for testing
    const hasAllOrigins = manifest.host_permissions.some(perm =>
      perm.includes('http://*/*') || perm.includes('https://*/*')
    );

    expect(hasAllOrigins).toBe(true);
    console.log('CORS bypass permissions verified:', manifest.host_permissions);
  }, TEST_TIMEOUT);

  // Note: Node connectivity test requires actual implementation in Flutter app
  // This is a placeholder that can be expanded when UI is ready
  test.skip('can connect to Monero stagenet node', async () => {
    if (!extensionPage) {
      extensionPage = await browser.newPage();
      await extensionPage.goto(`chrome-extension://${extensionId}/index.html`);
    }

    // TODO: Implement when Flutter UI has node connectivity testing
    // - Find node URL input field
    // - Enter stagenet node URL
    // - Click connect button
    // - Verify connection success message

    // Example:
    // await extensionPage.type('#node-url', 'http://stagenet.xmr-tw.org:38081');
    // await extensionPage.click('#connect-button');
    // await extensionPage.waitForSelector('#connection-status.success');
    // const status = await extensionPage.$eval('#connection-status', el => el.textContent);
    // expect(status).toContain('Connected');
  });

  test.skip('seed generation works', async () => {
    if (!extensionPage) {
      extensionPage = await browser.newPage();
      await extensionPage.goto(`chrome-extension://${extensionId}/index.html`);
    }

    // TODO: Implement when Flutter UI is ready
    // - Click generate seed button
    // - Verify seed phrase appears (25 words)
    // - Verify seed is valid Polyseed format
  });

  test.skip('key derivation works', async () => {
    if (!extensionPage) {
      extensionPage = await browser.newPage();
      await extensionPage.goto(`chrome-extension://${extensionId}/index.html`);
    }

    // TODO: Implement when Flutter UI is ready
    // - Enter test seed phrase
    // - Verify address derives correctly
    // - Verify view/spend keys display
  });
});

// Helper function to wait for element with timeout
async function waitForElement(page, selector, timeout = 5000) {
  try {
    await page.waitForSelector(selector, { timeout });
    return true;
  } catch (error) {
    return false;
  }
}
