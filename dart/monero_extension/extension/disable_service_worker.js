// Disable service worker - doesn't work with chrome-extension:// scheme
(function() {
  'use strict';

  window._flutter = window._flutter || {};
  window._flutter.buildConfig = window._flutter.buildConfig || {};

  // Strip service worker config from loader methods
  Object.defineProperty(window._flutter, 'loader', {
    set: function(loaderInstance) {
      this._loaderInstance = loaderInstance;

      const originalLoadEntrypoint = loaderInstance.loadEntrypoint;
      if (originalLoadEntrypoint) {
        loaderInstance.loadEntrypoint = function(config) {
          config = config || {};
          delete config.serviceWorker;
          delete config.serviceWorkerSettings;
          return originalLoadEntrypoint.call(this, config);
        };
      }

      const originalLoad = loaderInstance.load;
      if (originalLoad) {
        loaderInstance.load = function(config) {
          config = config || {};
          delete config.serviceWorker;
          delete config.serviceWorkerSettings;
          config.serviceWorkerSettings = undefined;
          return originalLoad.call(this, config);
        };
      }
    },
    get: function() {
      return this._loaderInstance;
    },
    configurable: true
  });
})();
