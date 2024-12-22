const puppeteer = require('puppeteer');
const path = require('path');
const fs = require('fs');

describe('Monero Operations E2E - Browser Extension', () => {
  let browser;
  let extensionPage;
  let extensionId;

  const EXTENSION_PATH = path.join(__dirname, '../../build/extension');
  const BUILD_TIMEOUT = 120000;
  const TEST_TIMEOUT = 30000;

  beforeAll(async () => {
    if (!fs.existsSync(EXTENSION_PATH)) {
      throw new Error(`Extension not found at: ${EXTENSION_PATH}. Run 'npm run build' first.`);
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

    console.log(`Extension loaded: ${extensionId}`);

    const popupUrl = `chrome-extension://${extensionId}/index.html`;
    extensionPage = await browser.newPage();

    extensionPage.on('console', msg => {
      if (msg.type() === 'error') {
        console.log(`[Browser error]:`, msg.text());
      }
    });

    await extensionPage.goto(popupUrl);
    await extensionPage.waitForSelector('flt-glass-pane', { timeout: 10000 });
    await new Promise(resolve => setTimeout(resolve, 3000));

    console.log('Extension page loaded');

  }, BUILD_TIMEOUT);

  afterAll(async () => {
    if (browser) {
      await browser.close();
    }
  });

  it('Should build and load the browser extension', async () => {
    expect(extensionId).toBeDefined();
    expect(extensionId).toMatch(/^[a-z]{32}$/);

    const hasFlutter = await extensionPage.evaluate(() => {
      return document.querySelector('flt-glass-pane') !== null;
    });
    expect(hasFlutter).toBe(true);

    const hasContent = await extensionPage.evaluate(() => {
      return document.body.textContent.length > 0;
    });
    expect(hasContent).toBe(true);
  }, TEST_TIMEOUT);

  it('Should have WASM modules loaded', async () => {
    const hasFlutterRenderer = await extensionPage.evaluate(() => {
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

    const hasMoneroContent = await extensionPage.evaluate(() => {
      const text = document.body.textContent;
      return text.includes('Monero') || text.includes('Wallet') || text.includes('Loading');
    });

    expect(hasMoneroContent).toBe(true);
  }, TEST_TIMEOUT);

  it('Should be able to execute code in extension context', async () => {
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
  }, TEST_TIMEOUT);
});
