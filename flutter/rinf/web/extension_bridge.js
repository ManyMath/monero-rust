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
