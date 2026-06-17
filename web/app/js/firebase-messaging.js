// FCM (Firebase Cloud Messaging) web glue for the darkrun web app.
//
// Gets an FCM registration token for THIS browser so the relay can push
// "a gate needs you" notifications to it — the remote half of notify-on-tick,
// for when you're away from the machine running the run. The Rust/wasm app
// calls `requestPushToken` (via wasm-bindgen) after the user opts in; the token
// is then POSTed to the relay's /devices endpoint.
//
// Requires: the user to grant notification permission, and the service worker
// `firebase-messaging-sw.js` (served from the app origin ROOT) to receive
// background pushes.
//
// Config: the PUBLIC web-app config (same project as firebase-login.js) plus the
// Web Push VAPID key (Firebase console → Project settings → Cloud Messaging →
// "Web Push certificates"). All public, safe to ship — fill the REPLACE_WITH_*
// values from the console.

import {
  initializeApp,
  getApps,
} from "https://www.gstatic.com/firebasejs/11.1.0/firebase-app.js";
import {
  getMessaging,
  getToken,
} from "https://www.gstatic.com/firebasejs/11.1.0/firebase-messaging.js";

const firebaseConfig = {
  apiKey: "REPLACE_WITH_FIREBASE_WEB_API_KEY",
  authDomain: "darkrun.firebaseapp.com",
  projectId: "darkrun",
  messagingSenderId: "REPLACE_WITH_FIREBASE_MESSAGING_SENDER_ID",
  appId: "REPLACE_WITH_FIREBASE_WEB_APP_ID",
};

// The Web Push VAPID public key (Cloud Messaging → Web Push certificates).
const VAPID_KEY = "REPLACE_WITH_FIREBASE_WEB_PUSH_VAPID_KEY";

// Reuse the already-initialized app if firebase-login.js created one.
function firebaseApp() {
  return getApps()[0] || initializeApp(firebaseConfig);
}

// Request notification permission and resolve to this browser's FCM token, or
// reject (→ Err on the Rust side) when unsupported or denied. Registers the
// background service worker so pushes arrive even when the tab isn't focused.
export async function requestPushToken() {
  if (!("Notification" in window) || !("serviceWorker" in navigator)) {
    throw new Error("push notifications are not supported in this browser");
  }
  const permission = await Notification.requestPermission();
  if (permission !== "granted") {
    throw new Error("notification permission " + permission);
  }
  const registration = await navigator.serviceWorker.register(
    "/firebase-messaging-sw.js",
  );
  const messaging = getMessaging(firebaseApp());
  const token = await getToken(messaging, {
    vapidKey: VAPID_KEY,
    serviceWorkerRegistration: registration,
  });
  if (!token) {
    throw new Error("Firebase returned no FCM token");
  }
  return token;
}
