/** Shared E2E test fixtures: wallet constants, signal helpers, node probe. */

// Primary test wallet with verified stagenet outputs
const HONKED_SEED = 'honked bagpipe alpine juicy faked afoot jostle claim cowl tunnel orphans negative pheasants feast jetting quote frown teeming cycling tribal womanly hills cottage daytime daytime';
const HONKED_ADDRESS = '58aWiYGUeqZc5idYcx31rYR58K1EVsCYkN6thrZppU1MGqMowPh1BYy4frVWH5RjGLPWthZy9sRGm5ZC4fgX44HUCmqtGUf';

// Test wallet used in core_functionality.test.js
const HEMLOCK_SEED = 'hemlock jubilee eden hacksaw boil superior inroads epoxy exhale orders cavernous second brunt saved richly lower upgrade hitched launching deepest mostly playful layout lower eden';
const HEMLOCK_ADDRESS = '569ubRY6tYfgF3VpxQumrUCRaEtdyyh6NG8sVD3YRVVJbK1jkpJ3zq8WHLijVzodQ22LxwkdWx7fS2a6JzaRGzkNU654PZu';

// BIP39 test mnemonic
const BIP39_TEST_MNEMONIC = 'color ranch color remove subway public water embrace before begin liberty fault';

// Synthetic unsigned TX hex for offline signing tests
const UNSIGNED_TX_HEX = '0010424242424242424242424242424242424242424242424242424242424242424200e1f5050000000002005f353861576959475565715a63356964596378333172595235384b3145567343596b4e367468725a707055314d47714d6f7750683142597934667256574835526a474c505774685a79397352476d355a433466675834344855436d71744755660040b743ba000000015f34384e5564684d5831455463356964596378333172595235384b3145567343596b4e367468725a707055314d47714d6f7750683142597934667256574835526a474c505774685a79397352476d355a43346667583434485543714463536a58015c356a10ea89d4ca34efbbafe4a820d89aa7f0356bf82670d228e44f67f6e30100eff78a2e0000000001aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa00bb8441d6bd13c0e4f3f0866ae17d8f31db964751e9577fcec87f1daefcec18a4010101010101010101010101010101010101010101010101010101010101010105050505050505050505050505050505050505050505050505050505050505050010a5d4e80000000000000000000000000000000050c30000000000000010d0860364646464646464646464646464646410bb8441d6bd13c0e4f3f0866ae17d8f31db964751e9577fcec87f1daefcec18a45c46896385ff4540620fbb4a359354add8603ce7fbb05aafca98c7d3f406762c857eed804ff087b97f87848f6493e87257a8c5203cb9f422f6e7a7d8a4d299f362485ef787a6ec0fabc79c76d19b87aa1d9381172fc3d14796448c6c44065ef285ce3cf603efcf45b599cce75369e854823864e471ad297d955f32db0ade7d425cc6b0877c06c9e62f3485e89a0e1f1bada60a990a70eb9dd0c22042bbcf6f93980052c1e22f4f2deb0956e62bc712140f2dd568413b02080de278c6a82fcb3de24e118f1a04aa0ad0032b9ab94e4952b5182b10820af934c0f5c80bf7be53a4a49de455dc6038523d5d447da6bc65a3c06454dd9f56f2d42afea0c880ce183f21908bd22ea2a33b290e50e4b60c107db274a4e8c7818c72afbb289e5b7ae90dd171c476742a5ebb9458ca0198bef0031ac79b5c6188fdfe6edf658749adb811bddd1857a540b1f59433e73308eec57aa84368cc6ef0809362023ac6bdd3482c52bf047c41c99269edae7bdd2a6527c213df89929e465647c9b52b0b2dd84d3a0559b76f74e64c3cd7f32e94b8fa98a7f4a009e6497aaa2200a7403dc00b1ef26440a777a0e2c880f8d10491d4435da40c08a8cdb40599b6d958ec8b161ef597fd32d621c1d87592fc6c1e4895121f8cd17e76c6bee017106c83c2ed893d3d6ac5ea64522efd11fe323d2ccc22a6320a9fae8252ab4a5f5e022b72b994f707a5ecf6f01ba7b538a53b59295bb0b83717f9c9299b97596c317e028735364743094704c71f41db8ea6a6811f019b1a08d6a960d44984701e43eea752ff9483ce02b482f1ebc441fef4d4e83a552c1e9559371011f0695495a8e729b21eaeea6685cb90b4ba5a6ad4bf86ffef1535b4237eba1ed085200f49b0e05fe0e71f76c5b01e8efe7091cfc4a4ce01365c95ceed5bb9fb36ef0c8137a0fdd6ee9fda0c35d2e0231af5f56fe4752c17edb2cd57f041940266a7d2f676260d7a8951d20026e98ab1e3b96aa14aa7cd6f4109752bcd4d77d8028085b01712d08c52d62f9124c065a5a4e04068d3590afbacf74e354e1fc2aaa9c845afaa32c34f230c8f2981750d0a77bb481eeda7e5204eb5c8552e455c3acecbcd7befcef113335074a85ecae6cbbc35d30de7e11a87ac6d03a2c696b351cf21dbedcd2c1e123e9fc8a4eb8f7ea7e0271ac3cb76fadbdf643ad8d4b2e941b16a208488f996cd3e3b1f064fdc697419e53c4a50c7936d3645265b4a683dca075c10c5ad074652a349e5aff58cd553b0226126e6dd9012d7c4d09a2b0851db158aea2638bc007c2454fda63555ee9f23fb37a204fc7b6fe7effc6820de222f4a2a118dc07966847f28454976485283424415985c30f0c3b44efe37c7c21bdf58db461d019cae38ad6fdf7b257e';

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
  UNSIGNED_TX_HEX,
  sendSignalAndWait,
  reloadExtensionPage,
  probeNode,
};
