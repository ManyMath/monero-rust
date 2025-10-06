/** Shared E2E test fixtures: wallet constants, signal helpers, node probe. */

// Primary test wallet with verified stagenet outputs
const HONKED_SEED = 'honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime';
const HONKED_ADDRESS = '58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf';

// Test wallet used in core_functionality.test.js
const HEMLOCK_SEED = 'hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden';
const HEMLOCK_ADDRESS = '569ubRY6tYfgF3VpxQumrUCRaEtdyyh6NG8sVD3YRVVJbK1jkpJ3zq8WHLijVzodQ22LxwkdWx7fS2a6JzaRGzkNU654PZu';

// BIP39 test mnemonic
const BIP39_TEST_MNEMONIC = 'color ranch color remove subway public water embrace before begin liberty fault';

/** Returns true if the stagenet node at `url` responds with HTTP 200. */
function probeNode(url = 'http://127.0.0.1:38081/get_info', timeoutMs = 3000) {
  const http = require('http');
  return new Promise((resolve) => {
    const req = http.get(url, { timeout: timeoutMs }, (res) => {
      res.resume();
      resolve(res.statusCode === 200);
    });
    req.on('error', () => resolve(false));
    req.on('timeout', () => { req.destroy(); resolve(false); });
  });
}

// ---------------------------------------------------------------------------
// Signal round-trip helper
// ---------------------------------------------------------------------------

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

module.exports = {
  HONKED_SEED, HONKED_ADDRESS,
  HEMLOCK_SEED, HEMLOCK_ADDRESS,
  BIP39_TEST_MNEMONIC,
  sendSignalAndWait,
  reloadExtensionPage,
  probeNode,
};
