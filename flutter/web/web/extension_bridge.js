window.wasmBindings = null;
window.wasmBindingsInitError = null;
window.wasmBindingsErrorCallback = null;
window.wasmBindingsReady = (async function initWasmBindings() {
  try {
    const wasmModule = await import('./pkg/monero_wasm.js');
    await wasmModule.default();
    await wasmModule.start_rust_runtime();

    const wrappedBindings = {};
    const originalRegister = wasmModule.register_rust_signal_callback.bind(wasmModule);
    wrappedBindings.register_rust_signal_callback = function registerWrappedRustSignalCallback(callback) {
      window.wasmBindingsErrorCallback = callback;
      return originalRegister(callback);
    };

    const emitBridgeError = (error) => {
      if (typeof window.wasmBindingsErrorCallback !== 'function') {
        return;
      }
      const message = error instanceof Error ? error.message : String(error);
      window.wasmBindingsErrorCallback(
        'ErrorResponse',
        JSON.stringify({
          error: message,
          error_code: 9000,
          error_hint: 'The request could not be parsed before it reached the Rust actor',
          error_transient: false,
        })
      );
    };

    for (const [key, value] of Object.entries(wasmModule)) {
      if (key === 'register_rust_signal_callback') {
        continue;
      }

      if (key.startsWith('send_') && typeof value === 'function') {
        const originalSend = value.bind(wasmModule);
        wrappedBindings[key] = function wrappedSignalSender(json) {
          try {
            return originalSend(json);
          } catch (error) {
            emitBridgeError(error);
            return undefined;
          }
        };
        continue;
      }

      if (typeof value === 'function') {
        wrappedBindings[key] = value.bind(wasmModule);
        continue;
      }

      wrappedBindings[key] = value;
    }

    window.wasmBindings = wrappedBindings;
    return wrappedBindings;
  } catch (error) {
    window.wasmBindingsInitError = error;
    console.error('Failed to initialize wasm bindings:', error);
    throw error;
  }
})();

window.extensionBridge = {
  isExtension: function() {
    return typeof chrome !== 'undefined' && chrome.runtime && chrome.runtime.id;
  },

  isSidePanel: function() {
    return !window.location.href.includes('mode=fullpage');
  },

  openFullPage: function() {
    if (chrome && chrome.runtime) {
      chrome.runtime.sendMessage({action: 'openFullPage'});
    }
  },

  openSidePanel: async function() {
    if (chrome && chrome.sidePanel) {
      try {
        const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
        await chrome.sidePanel.setOptions({ enabled: true });
        await chrome.sidePanel.open({ windowId: tab.windowId });
        await chrome.tabs.remove(tab.id);
      } catch (error) {
        console.error('Side panel error:', error);
      }
    }
  }
};
