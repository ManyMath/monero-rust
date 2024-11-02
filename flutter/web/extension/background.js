chrome.action.onClicked.addListener(async (tab) => {
  await chrome.sidePanel.open({ windowId: tab.windowId });
});

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.action === 'openFullPage') {
    chrome.windows.getCurrent(async (window) => {
      try {
        await chrome.tabs.create({
          url: chrome.runtime.getURL('index.html') + '?mode=fullpage',
          windowId: window.id
        });

        await chrome.sidePanel.setOptions({ enabled: false });

        setTimeout(async () => {
          await chrome.sidePanel.setOptions({ enabled: true });
        }, 300);

        sendResponse({ success: true });
      } catch (error) {
        sendResponse({ success: false, error: error.message });
      }
    });
    return true;
  }
});
