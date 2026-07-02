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
  linkWithPopup,
  GithubAuthProvider,
  OAuthProvider,
} from "https://www.gstatic.com/firebasejs/11.1.0/firebase-auth.js";

const firebaseConfig = {
  apiKey: "AIzaSyDhYi2DQAkbancuR71x3tqQhQ9AE3U29d8",
  authDomain: "darkrun.firebaseapp.com",
  projectId: "darkrun",
  appId: "1:32118591905:web:987db3ba09d6991b837be0",
};

const app = initializeApp(firebaseConfig);
const auth = getAuth(app);

// Build the Firebase Auth provider for `providerKey` ("github" | "gitlab").
function providerFor(providerKey) {
  if (providerKey === "gitlab") {
    // GitLab via Firebase generic OIDC — the provider id configured in console.
    // (Firebase requests `openid` itself.) The darkrun GitLab application must
    // ALLOW the exact scopes requested, else GitLab errors "requested scope is
    // invalid". Keep it minimal: `openid` (auto) + `read_api` (list projects,
    // read-only — NOT full `api`). So enable just those two on the GitLab app.
    const provider = new OAuthProvider("oidc.gitlab");
    provider.addScope("read_api");
    return provider;
  }
  const provider = new GithubAuthProvider();
  provider.addScope("read:user");
  // `repo` scope so the OAuth token can list the user's repositories.
  provider.addScope("repo");
  return provider;
}

// Reject `promise` after `ms` if it hasn't settled. signInWithPopup can hang
// forever when the popup completes but never hands a result back (e.g. the OIDC
// token exchange stalling in Firebase's auth handler after the user authorizes),
// which otherwise strands the app on "Signing in…" with no recovery. This turns
// that into a clear, retryable failure.
function withTimeout(promise, ms, message) {
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(message)), ms);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

// Map a Firebase auth error to a short, actionable message; falls back to the
// raw message. `providerKey` names the provider the user picked.
function friendlyAuthError(e, providerKey) {
  const label = providerKey === "gitlab" ? "GitLab" : "GitHub";
  const other = providerKey === "gitlab" ? "GitHub" : "GitLab";
  const code = (e && e.code) || "";
  if (code === "auth/account-exists-with-different-credential") {
    return `Your email is already on darkrun through ${other}. Sign in with ${other}, then add ${label} from your dashboard.`;
  }
  if (code === "auth/popup-closed-by-user" || code === "auth/cancelled-popup-request") {
    return `${label} sign-in was cancelled.`;
  }
  if (code === "auth/popup-blocked") {
    return "Your browser blocked the sign-in popup. Allow popups for this site, then try again.";
  }
  return (e && e.message) || `${label} sign-in failed.`;
}

// Open the sign-in popup for `providerKey`, guarded by a timeout and mapped to a
// friendly error. Shared by every sign-in entry point below.
async function signInPopup(providerKey) {
  const label = providerKey === "gitlab" ? "GitLab" : "GitHub";
  const other = providerKey === "gitlab" ? "GitHub" : "GitLab";
  try {
    return await withTimeout(
      signInWithPopup(auth, providerFor(providerKey)),
      120000,
      `${label} sign-in didn't finish. Close the popup and try again; if ${label} keeps stalling, sign in with ${other} instead.`,
    );
  } catch (e) {
    throw new Error(friendlyAuthError(e, providerKey));
  }
}

// Sign in with `providerKey` ("github" | "gitlab") and resolve to the Firebase
// ID token string. Rejects (→ Err on the Rust side) on cancel/failure.
//
// This is the CLI-login bridge path: the CLI only needs the Firebase ID token
// (deposited under its nonce), so this returns just that.
export async function signInAndGetToken(providerKey) {
  const result = await signInPopup(providerKey);
  return await result.user.getIdToken();
}

// Sign in and resolve to BOTH tokens, JSON-encoded:
//   { "idToken": "...", "accessToken": "...", "provider": "github" }
//
// The standalone dashboard needs the **provider OAuth access token** (not the
// Firebase ID token) to list repos through the darkrun-web `/api/repos` proxy.
// Firebase surfaces it on the sign-in result's credential. `accessToken` is the
// empty string if the provider didn't return one (the dashboard then degrades
// to "no repos to show" rather than failing the sign-in).
export async function signInForDashboard(providerKey) {
  const result = await signInPopup(providerKey);
  return credentialJson(result, providerKey);
}

// LINK a second provider ("github" | "gitlab") to the CURRENTLY signed-in
// Firebase account, so ONE darkrun account spans both GitHub and GitLab. Resolves
// to the same `{ idToken, accessToken, provider }` JSON as signInForDashboard —
// the dashboard then lists the newly-linked provider's repos alongside the first.
//
// Rejects (→ Err on the Rust side) if no one is signed in, the popup is
// cancelled, the provider is already linked (`auth/provider-already-linked`), or
// that provider identity already belongs to a DIFFERENT darkrun account
// (`auth/credential-already-in-use`); the message is surfaced to the user.
export async function linkProvider(providerKey) {
  const user = auth.currentUser;
  if (!user) throw new Error("Sign in before linking another account.");
  const label = providerKey === "gitlab" ? "GitLab" : "GitHub";
  try {
    const result = await withTimeout(
      linkWithPopup(user, providerFor(providerKey)),
      120000,
      `Linking ${label} didn't finish. Close the popup and try again.`,
    );
    return credentialJson(result, providerKey);
  } catch (e) {
    throw new Error(friendlyAuthError(e, providerKey));
  }
}

// Extract { idToken, accessToken, provider } from a sign-in / link result.
// `accessToken` is the empty string if the provider returned none (the dashboard
// then degrades to "no repos to show" for that identity rather than failing).
async function credentialJson(result, providerKey) {
  const idToken = await result.user.getIdToken();
  const credential =
    providerKey === "gitlab"
      ? OAuthProvider.credentialFromResult(result)
      : GithubAuthProvider.credentialFromResult(result);
  const accessToken = (credential && credential.accessToken) || "";
  return JSON.stringify({ idToken, accessToken, provider: providerKey });
}
