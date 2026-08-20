// Injected into every page loaded in the shell (local splash + remote
// pwa.felyne.app). Exposes a narrow bridge as window.__FELYNE_SHELL__ and
// routes external links to the system browser. Fail-closed: if the Tauri IPC
// is unavailable the page simply behaves as a normal webview with no shell
// features.

(function () {
  var VERSION = '__SHELL_VERSION__';
  var PLATFORM = '__SHELL_PLATFORM__';

  var internals = window.__TAURI_INTERNALS__;
  var invoke = internals
    ? function (cmd, args) {
        return internals.invoke(cmd, args);
      }
    : function () {
        return Promise.reject(new Error('tauri ipc unavailable'));
      };

  var bridge = {
    isShell: true,
    version: VERSION,
    platform: PLATFORM,
    notify: function (title, body, data) {
      return invoke('shell_notify', { title: title, body: body, data: data || {} });
    },
    openExternal: function (url) {
      return invoke('shell_open_external', { url: url });
    },
    requestNotificationPermission: function () {
      return invoke('shell_request_notification_permission');
    },
    notificationsPermission: function () {
      return invoke('shell_notifications_permission');
    },
  };

  Object.defineProperty(window, '__FELYNE_SHELL__', {
    value: bridge,
    configurable: false,
    writable: false,
  });

  // Off-origin and target=_blank links leave the shell and open in the system
  // browser instead of navigating the whole window to an attacker origin.
  document.addEventListener(
    'click',
    function (event) {
      var anchor =
        event.target && event.target.closest ? event.target.closest('a[href]') : null;
      if (!anchor) return;
      var href = anchor.getAttribute('href');
      if (!href) return;
      var target;
      try {
        target = new URL(href, window.location.href);
      } catch (e) {
        return;
      }
      var offOrigin = target.origin !== window.location.origin;
      var opensNewTab = anchor.target === '_blank' || anchor.hasAttribute('download');
      if (offOrigin || opensNewTab) {
        event.preventDefault();
        event.stopPropagation();
        invoke('shell_open_external', { url: target.href });
      }
    },
    true
  );
})();