// Local splash: probe the remote app, then hand off. On failure show an
// explicit offline state with a Retry button instead of a stuck blank window.

var PWA_URL = 'https://pwa.felyne.app';
var PROBE_TIMEOUT_MS = 8000;

var statusEl = document.getElementById('status');
var retryEl = document.getElementById('retry');

function setStatus(text) {
  statusEl.textContent = text;
}

async function reachable() {
  var controller = new AbortController();
  var timer = setTimeout(function () {
    controller.abort();
  }, PROBE_TIMEOUT_MS);
  try {
    await fetch(PWA_URL, { method: 'HEAD', mode: 'no-cors', signal: controller.signal });
    return true;
  } catch (e) {
    return false;
  } finally {
    clearTimeout(timer);
  }
}

async function launch() {
  retryEl.classList.add('hidden');
  setStatus('Connecting\u2026');
  var ok = await reachable();
  if (!ok) {
    setStatus('Can\u2019t reach pwa.felyne.app. Check your connection.');
    retryEl.classList.remove('hidden');
    return;
  }
  setStatus('Loading\u2026');
  window.location.assign(PWA_URL);
}

retryEl.addEventListener('click', launch);
launch();