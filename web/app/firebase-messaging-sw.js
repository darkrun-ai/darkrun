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
// with a `notification` block (title/body from the host's gate event).
messaging.onBackgroundMessage((payload) => {
  const n = payload.notification || {};
  self.registration.showNotification(n.title || "darkrun", {
    body: n.body || "",
    icon: "/assets/favicon.png",
    tag: "darkrun-gate",
  });
});
