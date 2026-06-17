// FCM background service worker for the darkrun web app.
//
// Receives pushes when the app's tab isn't focused and shows the notification.
// MUST be served from the app origin ROOT (`/firebase-messaging-sw.js`) — a
// service worker can only control its own path and below. The deploy-app
// workflow copies this into the published dist root.
//
// Service workers can't use ES module imports reliably across browsers, so this
// uses the Firebase compat SDK via importScripts (the standard FCM SW shape).
// The config is the same PUBLIC web-app config as firebase-messaging.js — fill
// the REPLACE_WITH_* values from the Firebase console.

importScripts(
  "https://www.gstatic.com/firebasejs/11.1.0/firebase-app-compat.js",
);
importScripts(
  "https://www.gstatic.com/firebasejs/11.1.0/firebase-messaging-compat.js",
);

firebase.initializeApp({
  apiKey: "REPLACE_WITH_FIREBASE_WEB_API_KEY",
  authDomain: "darkrun.firebaseapp.com",
  projectId: "darkrun",
  messagingSenderId: "REPLACE_WITH_FIREBASE_MESSAGING_SENDER_ID",
  appId: "REPLACE_WITH_FIREBASE_WEB_APP_ID",
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
