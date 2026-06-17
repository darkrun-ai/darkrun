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
// Config: the PUBLIC web-app config (same project as firebase-login.js) is set
// below. The one value still to fill is the Web Push VAPID key (Firebase console
// → Project settings → Cloud Messaging → "Web Push certificates" → the public
// "Key pair"); it has no fetch API, so it's pasted in by hand. All public.

import {
  initializeApp,
  getApps,
} from "https://www.gstatic.com/firebasejs/11.1.0/firebase-app.js";
import {
  getMessaging,
  getToken,
} from "https://www.gstatic.com/firebasejs/11.1.0/firebase-messaging.js";

const firebaseConfig = {
  apiKey: "AIzaSyDhYi2DQAkbancuR71x3tqQhQ9AE3U29d8",
  authDomain: "darkrun.firebaseapp.com",
  projectId: "darkrun",
  messagingSenderId: "32118591905",
  appId: "1:32118591905:web:987db3ba09d6991b837be0",
};

// The Web Push VAPID public key (Cloud Messaging → Web Push certificates).
const VAPID_KEY = "BFxSAhVFAvO5fRDcIpTw3fAbVl7WPnjy4x9S-Pd9r8_zowSZ6FNE40r7svQcLcMdEZ-PvplfaT60Kq5TPjXTjwI";

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
