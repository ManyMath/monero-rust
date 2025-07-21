// Web Worker that hosts the Rust WASM module.
// All heavy crypto operations run here, off the main/UI thread.

import init, * as wasm from './monero_wasm.js';

await init();

// Register callback: when Rust sends a signal, post it to the main thread.
wasm.register_rust_signal_callback((typeName, json) => {
  self.postMessage({ type: 'rust_signal', typeName, json });
});

// Start the Rust async runtime (spawns actor system).
await wasm.start_rust_runtime();

self.postMessage({ type: 'ready' });

// Listen for dart signals from the main thread.
self.onmessage = (e) => {
  const { type, signalName, json } = e.data;
  if (type === 'dart_signal') {
    try {
      const fn = wasm[signalName];
      if (fn) {
        fn(json);
      } else {
        self.postMessage({
          type: 'error',
          signalName,
          error: `Unknown signal function: ${signalName}`,
        });
      }
    } catch (err) {
      self.postMessage({
        type: 'error',
        signalName,
        error: err.message || String(err),
      });
    }
  }
};
