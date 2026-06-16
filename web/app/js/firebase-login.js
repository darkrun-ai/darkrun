// Firebase Auth sign-in glue for the darkrun web app.
//
// The Firebase SDK is JavaScript, so this thin ES module wraps the sign-in flow
// into one function the Rust/wasm app calls via wasm-bindgen. It signs in with
// the chosen provider and returns the user's Firebase ID token — the token the
// relay verifies and `darkrun login` stores.
//
// The config values below are the PUBLIC web-app config (safe to ship — they're
// not secrets); fill apiKey + appId from the Firebase console
// (Project settings → your web app). projectId/authDomain are the `darkrun`
// project's. GitHub is a built-in provider; GitLab is a generic OIDC provider
// (`oidc.gitlab`) you configure in Firebase Auth.

import { initializeApp } from "https://www.gstatic.com/firebasejs/11.1.0/firebase-app.js";
import {
  getAuth,
  signInWithPopup,
  GithubAuthProvider,
  OAuthProvider,
} from "https://www.gstatic.com/firebasejs/11.1.0/firebase-auth.js";

const firebaseConfig = {
  apiKey: "REPLACE_WITH_FIREBASE_WEB_API_KEY",
  authDomain: "darkrun.firebaseapp.com",
  projectId: "darkrun",
  appId: "REPLACE_WITH_FIREBASE_WEB_APP_ID",
};

const app = initializeApp(firebaseConfig);
const auth = getAuth(app);

// Sign in with `providerKey` ("github" | "gitlab") and resolve to the Firebase
// ID token string. Rejects (→ Err on the Rust side) on cancel/failure.
export async function signInAndGetToken(providerKey) {
  let provider;
  if (providerKey === "gitlab") {
    // GitLab via Firebase generic OIDC — the provider id configured in console.
    provider = new OAuthProvider("oidc.gitlab");
  } else {
    provider = new GithubAuthProvider();
    provider.addScope("read:user");
  }
  const result = await signInWithPopup(auth, provider);
  return await result.user.getIdToken();
}
