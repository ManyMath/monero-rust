// Background script for MoneroExtension
// Opens the extension in a full browser tab when the extension icon is clicked

chrome.action.onClicked.addListener(() => {
  chrome.tabs.create({
    url: chrome.runtime.getURL('index.html')
  });
});
