const childProcess = require('child_process');
const fs = require('fs');
const http = require('http');
const net = require('net');
const os = require('os');
const path = require('path');
const puppeteer = require('puppeteer');

const { sendSignalAndWait, waitForScanCompletion } = require('./fixtures');

const SEED = 'velvet lymph giddy number token physics poetry unquoted nibs useful sabotage limits benches lifestyle eden nitrogen anvil fewest avoid batch vials washing fences goat unquoted';
const STANDARD_ADDRESS = '42ey1afDFnn4886T7196doS9GPMzexD9gXpsZJDwVjeRVdFCSoHnv7KPbBeGpzJBzHRCAs9UxqeoyFQMYbqSWYTfJJQAWDm';
const TRANSFER_AMOUNT_ATOMIC = 1_000_000_000_000;
const BLOCKS_TO_MINE = 80;
const RING_SIZE = 16;

function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

async function waitForPort(port, timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await canConnect(port)) {
      return;
    }
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error(`port ${port} did not open within ${timeoutMs}ms`);
}

function canConnect(port) {
  return new Promise(resolve => {
    const socket = net.createConnection({ host: '127.0.0.1', port });
    socket.setTimeout(250);
    socket.on('connect', () => {
      socket.destroy();
      resolve(true);
    });
    socket.on('error', () => resolve(false));
    socket.on('timeout', () => {
      socket.destroy();
      resolve(false);
    });
  });
}

function rpc(port, method, params = {}, timeoutMs = 90000) {
  const payload = JSON.stringify({
    jsonrpc: '2.0',
    id: '0',
    method,
    params,
  });

  return new Promise((resolve, reject) => {
    const req = http.request(
      {
        hostname: '127.0.0.1',
        port,
        path: '/json_rpc',
        method: 'POST',
        timeout: timeoutMs,
        headers: {
          'Content-Type': 'application/json',
          'Content-Length': Buffer.byteLength(payload),
        },
      },
      res => {
        let body = '';
        res.on('data', chunk => {
          body += chunk;
        });
        res.on('end', () => {
          if (res.statusCode !== 200) {
            reject(new Error(`${method} failed with HTTP ${res.statusCode}: ${body}`));
            return;
          }
          try {
            const parsed = JSON.parse(body);
            if (parsed.error) {
              reject(new Error(`${method} returned error: ${JSON.stringify(parsed.error)}`));
              return;
            }
            resolve(parsed.result || {});
          } catch (error) {
            reject(new Error(`${method} returned invalid JSON: ${error.message}`));
          }
        });
      }
    );
    req.on('error', reject);
    req.on('timeout', () => {
      req.destroy(new Error(`${method} timed out after ${timeoutMs}ms`));
    });
    req.write(payload);
    req.end();
  });
}

function spawnLogged(command, args, logPath) {
  const log = fs.openSync(logPath, 'w');
  const child = childProcess.spawn(command, args, {
    stdio: ['ignore', log, log],
  });
  child._logFd = log;
  return child;
}

async function stopProcesses(processes) {
  for (const child of [...processes].reverse()) {
    if (!child.killed && child.exitCode === null) {
      child.kill('SIGTERM');
    }
  }
  for (const child of [...processes].reverse()) {
    await new Promise(resolve => {
      if (child.exitCode !== null) {
        resolve();
        return;
      }
      const timer = setTimeout(() => {
        if (child.exitCode === null) {
          child.kill('SIGKILL');
        }
        resolve();
      }, 10000);
      child.once('exit', () => {
        clearTimeout(timer);
        resolve();
      });
    });
    if (child._logFd !== undefined) {
      fs.closeSync(child._logFd);
    }
  }
}

async function createRegtestHarness(moneroBinDir) {
  const monerod = path.join(moneroBinDir, 'monerod');
  const walletRpc = path.join(moneroBinDir, 'monero-wallet-rpc');
  if (!fs.existsSync(monerod) || !fs.existsSync(walletRpc)) {
    throw new Error(`MONERO_BIN_DIR must contain monerod and monero-wallet-rpc: ${moneroBinDir}`);
  }

  const workDir = fs.mkdtempSync(path.join(os.tmpdir(), 'monero-browser-regtest-'));
  const daemonRpc = await freePort();
  const daemonP2p = await freePort();
  const daemonZmq = await freePort();
  const hotRpc = await freePort();
  const coldRpc = await freePort();
  const processes = [];

  processes.push(spawnLogged(monerod, [
    '--regtest',
    '--fixed-difficulty', '1',
    '--p2p-bind-port', String(daemonP2p),
    '--rpc-bind-port', String(daemonRpc),
    '--zmq-rpc-bind-port', String(daemonZmq),
    '--non-interactive',
    '--offline',
    '--disable-dns-checkpoints',
    '--check-updates', 'disabled',
    '--rpc-ssl', 'disabled',
    '--rpc-access-control-origins', '*',
    '--data-dir', path.join(workDir, 'daemon'),
    '--log-level', '1',
    '--no-igd',
    '--hide-my-port',
  ], path.join(workDir, 'monerod.log')));
  await waitForPort(daemonRpc);

  for (const [name, port, extra] of [
    ['hot', hotRpc, ['--daemon-address', `127.0.0.1:${daemonRpc}`]],
    ['cold', coldRpc, ['--offline']],
  ]) {
    const walletDir = path.join(workDir, name);
    fs.mkdirSync(walletDir);
    processes.push(spawnLogged(walletRpc, [
      '--wallet-dir', walletDir,
      '--rpc-bind-ip', '127.0.0.1',
      '--rpc-bind-port', String(port),
      '--rpc-ssl', 'disabled',
      '--daemon-ssl', 'disabled',
      '--log-level', '1',
      '--allow-mismatched-daemon-version',
      '--disable-rpc-login',
      ...extra,
    ], path.join(workDir, `${name}.log`)));
  }
  await waitForPort(hotRpc);
  await waitForPort(coldRpc);

  return {
    workDir,
    daemonRpc,
    hotRpc,
    coldRpc,
    processes,
    async stop() {
      await stopProcesses(processes);
      fs.rmSync(workDir, { recursive: true, force: true });
    },
  };
}

async function prepareWallet2UnsignedTxset(harness) {
  const coldRestore = await rpc(harness.coldRpc, 'restore_deterministic_wallet', {
    filename: 'cold',
    password: '',
    seed: SEED,
    restore_height: 0,
    autosave_current: true,
  });
  expect(coldRestore.address).toBe(STANDARD_ADDRESS);

  const viewKey = (await rpc(harness.coldRpc, 'query_key', { key_type: 'view_key' })).key;
  await rpc(harness.hotRpc, 'generate_from_keys', {
    filename: 'hot',
    password: '',
    address: STANDARD_ADDRESS,
    viewkey: viewKey,
    restore_height: 0,
    autosave_current: true,
  });

  await rpc(harness.daemonRpc, 'generateblocks', {
    wallet_address: STANDARD_ADDRESS,
    amount_of_blocks: BLOCKS_TO_MINE,
  });
  await rpc(harness.hotRpc, 'refresh');

  const outputsExport = await rpc(harness.hotRpc, 'export_outputs', { all: true });
  const outputsImport = await rpc(harness.coldRpc, 'import_outputs', {
    outputs_data_hex: outputsExport.outputs_data_hex,
  });
  expect(outputsImport.num_imported).toBe(BLOCKS_TO_MINE);

  const keyImages = await rpc(harness.coldRpc, 'export_key_images', { all: true });
  expect(keyImages.signed_key_images).toHaveLength(BLOCKS_TO_MINE);
  await rpc(harness.hotRpc, 'import_key_images', {
    signed_key_images: keyImages.signed_key_images,
    offset: keyImages.offset,
  });

  const transfer = await rpc(harness.hotRpc, 'transfer', {
    destinations: [{ address: STANDARD_ADDRESS, amount: TRANSFER_AMOUNT_ATOMIC }],
    ring_size: RING_SIZE,
    get_tx_key: false,
  });
  const referenceSigned = await rpc(harness.coldRpc, 'sign_transfer', {
    unsigned_txset: transfer.unsigned_txset,
  });

  return {
    unsignedTxsetHex: transfer.unsigned_txset,
    referenceSignedTxsetHex: referenceSigned.signed_txset,
    coldKeysHex: fs
      .readFileSync(path.join(harness.workDir, 'cold', 'cold.keys'))
      .toString('hex'),
  };
}

describe('Wallet2 Regtest Browser UAT', () => {
  let browser;
  let extPage;
  let harness;
  let skipReason = null;
  const EXT_PATH = path.join(__dirname, '../../build/extension');

  beforeAll(async () => {
    const moneroBinDir = process.env.MONERO_BIN_DIR;
    if (!moneroBinDir) {
      skipReason = 'MONERO_BIN_DIR is not set';
      return;
    }
    if (!fs.existsSync(EXT_PATH)) {
      throw new Error(`Extension not found at: ${EXT_PATH}. Run 'npm run build' first.`);
    }

    harness = await createRegtestHarness(moneroBinDir);

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
    const target = browser.targets().find(t => /^chrome-extension:\/\/([a-z]{32})/.test(t.url()));
    const match = target?.url().match(/^chrome-extension:\/\/([a-z]{32})/);
    if (!match) {
      throw new Error('Could not determine extension ID');
    }

    extPage = await browser.newPage();
    await extPage.goto(`chrome-extension://${match[1]}/index.html`);
    await extPage.waitForSelector('flt-glass-pane', { timeout: 10000 });
    await extPage.waitForFunction(
      () => !!window.wasmBindings && typeof window.wasmBindings.send_sign_unsigned_transaction_request === 'function',
      { timeout: 15000 }
    );
  }, 120000);

  afterAll(async () => {
    if (browser) {
      await browser.close();
    }
    if (harness) {
      await harness.stop();
    }
  });

  it('signs a fresh wallet2 unsigned txset in browser and submits the rebuilt signed txset', async () => {
    if (skipReason) {
      console.log(`REGTEST-UAT-01 skipped: ${skipReason}`);
      return;
    }

    const fixture = await prepareWallet2UnsignedTxset(harness);

    const importedCold = await sendSignalAndWait(
      extPage,
      'send_import_keys_file_request',
      JSON.stringify({ file_bytes_hex: fixture.coldKeysHex, password: '' }),
      'ImportKeysFileResponse',
      10000
    );
    expect(importedCold.success).toBe(true);
    expect(importedCold.watch_only).toBe(false);
    expect(importedCold.spend_secret_key).toMatch(/^[0-9a-f]{64}$/);
    expect(importedCold.view_secret_key).toMatch(/^[0-9a-f]{64}$/);

    const metadata = await sendSignalAndWait(
      extPage,
      'send_extract_signed_txset_request',
      JSON.stringify({
        data_hex: fixture.referenceSignedTxsetHex,
        view_key_hex: importedCold.view_secret_key,
      }),
      'SignedTxSetExtractedResponse',
      10000
    );
    expect(metadata.success).toBe(true);
    expect(metadata.transactions).toHaveLength(1);
    expect(metadata.key_images).toHaveLength(BLOCKS_TO_MINE);

    const signed = await sendSignalAndWait(
      extPage,
      'send_sign_unsigned_transaction_request',
      JSON.stringify({
        seed: importedCold.mnemonic ?? '',
        unsigned_tx_hex: fixture.unsignedTxsetHex,
        network: 'mainnet',
        spend_secret_key_hex: importedCold.spend_secret_key,
        view_secret_key_hex: importedCold.view_secret_key,
      }),
      'TransactionSignedOfflineResponse',
      60000
    );
    expect(signed.success).toBe(true);
    expect(signed.tx_id).toMatch(/^[0-9a-f]{64}$/);
    expect(signed.tx_blob).toMatch(/^[0-9a-f]+$/);
    expect(signed.spent_key_images).toHaveLength(2);

    const built = await sendSignalAndWait(
      extPage,
      'send_build_signed_txset_request',
      JSON.stringify({
        unsigned_txset_hex: fixture.unsignedTxsetHex,
        view_key_hex: importedCold.view_secret_key,
        tx_blob_hex: signed.tx_blob,
        key_images: metadata.key_images,
        tx_key_images: metadata.tx_key_images,
      }),
      'SignedTxSetBuiltResponse',
      10000
    );
    expect(built.success).toBe(true);
    expect(built.signed_txset_hex).toMatch(/^4d6f6e65726f207369676e65642074782073657405/);

    const submitted = await rpc(harness.hotRpc, 'submit_transfer', {
      tx_data_hex: built.signed_txset_hex,
    });
    expect(submitted.tx_hash_list).toEqual([signed.tx_id]);
    await rpc(harness.daemonRpc, 'flush_txpool', { txids: [signed.tx_id] });
  }, 240000);

  it('scans fresh regtest outputs in browser and validates app-native do-not-relay broadcast', async () => {
    if (skipReason) {
      console.log(`REGTEST-UAT-02 skipped: ${skipReason}`);
      return;
    }

    const keys = await sendSignalAndWait(
      extPage,
      'send_derive_keys_request',
      JSON.stringify({ seed: SEED, network: 'mainnet' }),
      'KeysDerivedResponse',
      10000
    );
    expect(keys.success).toBe(true);
    expect(keys.secret_view_key).toMatch(/^[0-9a-f]{64}$/);
    expect(keys.public_spend_key).toMatch(/^[0-9a-f]{64}$/);

    const viewOnlySeed = `viewonly:${keys.secret_view_key}:${keys.public_spend_key}`;
    const scanResp = await waitForScanCompletion(
      extPage,
      JSON.stringify({
        node_url: `http://127.0.0.1:${harness.daemonRpc}`,
        start_height: 0,
        seed: viewOnlySeed,
        network: 'mainnet',
        account_lookahead: 1,
        subaddress_lookahead: 0,
        passphrase: '',
        bip39_account_index: 0,
        allow_insecure_http: true,
      }),
      120000
    );
    expect(scanResp.is_scanning).toBe(false);
    expect(scanResp.daemon_height).toBeGreaterThanOrEqual(BLOCKS_TO_MINE);

    const balance = await sendSignalAndWait(
      extPage,
      'send_get_balance_request',
      '{}',
      'BalanceResponse',
      10000
    );
    expect(balance.confirmed).toBeGreaterThan(TRANSFER_AMOUNT_ATOMIC);

    const unsigned = await sendSignalAndWait(
      extPage,
      'send_create_unsigned_transaction_request',
      JSON.stringify({
        node_url: `http://127.0.0.1:${harness.daemonRpc}`,
        view_key_hex: keys.secret_view_key,
        pub_spend_key_hex: keys.public_spend_key,
        network: 'mainnet',
        recipients: [{ address: STANDARD_ADDRESS, amount: TRANSFER_AMOUNT_ATOMIC }],
        max_fee_per_weight: 2_000_000,
      }),
      'UnsignedTransactionCreatedResponse',
      60000
    );
    expect(unsigned.success).toBe(true);
    expect(unsigned.unsigned_tx_hex).toMatch(/^[0-9a-f]+$/);
    expect(unsigned.fee).toBeGreaterThan(0);

    const signed = await sendSignalAndWait(
      extPage,
      'send_sign_unsigned_transaction_request',
      JSON.stringify({
        seed: SEED,
        unsigned_tx_hex: unsigned.unsigned_tx_hex,
        network: 'mainnet',
      }),
      'TransactionSignedOfflineResponse',
      60000
    );
    expect(signed.success).toBe(true);
    expect(signed.tx_id).toMatch(/^[0-9a-f]{64}$/);
    expect(signed.tx_blob).toMatch(/^[0-9a-f]+$/);
    expect(signed.fee).toBe(unsigned.fee);
    expect(signed.spent_key_images.length).toBeGreaterThan(0);

    const broadcast = await sendSignalAndWait(
      extPage,
      'send_broadcast_transaction_request',
      JSON.stringify({
        node_url: `http://127.0.0.1:${harness.daemonRpc}`,
        tx_blob: signed.tx_blob,
        spent_output_hashes: [],
        tx_id: signed.tx_id,
        spent_key_images: signed.spent_key_images,
        do_not_relay: true,
      }),
      'TransactionBroadcastResponse',
      30000
    );
    expect(broadcast.success).toBe(true);
    expect(broadcast.tx_id).toBe(signed.tx_id);
    expect(broadcast.is_retryable).toBe(false);
    expect(broadcast.is_double_spend).toBe(false);

    await rpc(harness.daemonRpc, 'flush_txpool', { txids: [signed.tx_id] });
  }, 240000);

});
