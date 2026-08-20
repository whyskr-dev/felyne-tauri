// Injected into every page loaded in the shell (local splash + remote
// felyne.app). Exposes a narrow bridge as window.__FELYNE_SHELL__ and
// routes external links to the system browser. Fail-closed: if the Tauri IPC
// is unavailable the page simply behaves as a normal webview with no shell
// features.

(function () {
  var VERSION = '__SHELL_VERSION__';
  var PLATFORM = '__SHELL_PLATFORM__';
  var APP_URL = '__FELYNE_APP_URL__';

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
    appUrl: APP_URL,
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
    checkForUpdate: function () {
      return invoke('shell_check_for_update');
    },
    installUpdate: function () {
      return invoke('shell_install_update');
    },
    onUpdateAvailable: function (handler) {
      return internals.event.listen('shell:update-available', function (event) {
        handler(event.payload);
      });
    },
    onUpdateInstalled: function (handler) {
      return internals.event.listen('shell:update-installed', function (event) {
        handler(event.payload);
      });
    },
  };

  // Push notifications (APNs on iOS / FCM on Android) via the
  // tauri-plugin-mobile-push plugin. Commands are no-ops on desktop. iOS
  // event delivery is currently limited by the plugin; the native banner
  // still shows, and the web app's Realtime + notify() path covers in-app
  // messages.
  bridge.push = {
    requestPermission: function () {
      return invoke('plugin:mobile-push|request_permission');
    },
    getToken: function () {
      return invoke('plugin:mobile-push|get_token');
    },
  };

  // Mirrors @tauri-apps/api's addPluginListener against the injected
  // __TAURI_INTERNALS__ (the web app has no bundled frontend to import it
  // from). Registering the listener first lets the plugin start delivering.
  function addPluginListener(plugin, event, handler) {
    return internals
      .invoke('plugin:' + plugin + '|register_listener', { event: event })
      .then(function () {
        return internals.event.listen('plugin:' + plugin + '|' + event, handler);
      });
  }

  bridge.onPushNotification = function (cb) {
    return addPluginListener('mobile-push', 'notification-received', function (e) {
      cb(e.payload);
    });
  };
  bridge.onPushTapped = function (cb) {
    return addPluginListener('mobile-push', 'notification-tapped', function (e) {
      cb(e.payload);
    });
  };
  bridge.onTokenRefresh = function (cb) {
    return addPluginListener('mobile-push', 'token-received', function (e) {
      cb(e.payload);
    });
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