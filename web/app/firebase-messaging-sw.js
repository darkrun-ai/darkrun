// FCM background service worker for the darkrun web app.
//
// Receives pushes when the app's tab isn't focused and shows the notification.
// MUST be served from the app origin ROOT (`/firebase-messaging-sw.js`) — a
// service worker can only control its own path and below. The deploy-app
// workflow copies this into the published dist root.
//
// Service workers can't use ES module imports reliably across browsers, so this
// uses the Firebase compat SDK via importScripts (the standard FCM SW shape).
// The config is the project's PUBLIC web-app config (same as
// firebase-messaging.js); the SW needs no VAPID key.

importScripts(
  "https://www.gstatic.com/firebasejs/11.1.0/firebase-app-compat.js",
);
importScripts(
  "https://www.gstatic.com/firebasejs/11.1.0/firebase-messaging-compat.js",
);

firebase.initializeApp({
  apiKey: "AIzaSyDhYi2DQAkbancuR71x3tqQhQ9AE3U29d8",
  authDomain: "darkrun.firebaseapp.com",
  projectId: "darkrun",
  messagingSenderId: "32118591905",
  appId: "1:32118591905:web:987db3ba09d6991b837be0",
});

const messaging = firebase.messaging();

// Render a background push as an OS notification. The relay sends FCM messages
// with a `notification` block (title/body from the host's gate event) and, for a
// gate on a specific run, a click target (`webpush.fcm_options.link`, mirrored
// into `webpush.data.link`) pointing at `app.darkrun.ai/runs/<slug>`. Stash the
// link in the notification `data` so the `notificationclick` handler can open the
// live run.
messaging.onBackgroundMessage((payload) => {
  const n = payload.notification || {};
  const link =
    (payload.fcmOptions && payload.fcmOptions.link) ||
    (payload.data && payload.data.link) ||
    "/";
  self.registration.showNotification(n.title || "darkrun", {
    body: n.body || "",
    icon: "/assets/favicon.png",
    tag: "darkrun-gate",
    data: { link },
  });
});

// Open (or focus) the run's live view when the operator taps the notification.
// Overriding onBackgroundMessage bypasses FCM's default click handling, so wire
// it explicitly: focus an existing darkrun tab if one is open (navigating it to
// the run), else open the run link in a new window.
self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const link = (event.notification.data && event.notification.data.link) || "/";
  event.waitUntil(
    self.clients
      .matchAll({ type: "window", includeUncontrolled: true })
      .then((clientList) => {
        for (const client of clientList) {
          if ("focus" in client) {
            if ("navigate" in client) {
              client.navigate(link);
            }
            return client.focus();
          }
        }
        if (self.clients.openWindow) {
          return self.clients.openWindow(link);
        }
      }),
  );
});
