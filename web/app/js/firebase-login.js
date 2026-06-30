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

// Sign in with `providerKey` ("github" | "gitlab") and resolve to the Firebase
// ID token string. Rejects (→ Err on the Rust side) on cancel/failure.
//
// This is the CLI-login bridge path: the CLI only needs the Firebase ID token
// (deposited under its nonce), so this returns just that — unchanged.
export async function signInAndGetToken(providerKey) {
  const result = await signInWithPopup(auth, providerFor(providerKey));
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
  const result = await signInWithPopup(auth, providerFor(providerKey));
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
// (`auth/credential-already-in-use`) — the message is surfaced to the user.
export async function linkProvider(providerKey) {
  const user = auth.currentUser;
  if (!user) throw new Error("Sign in before linking another account.");
  const result = await linkWithPopup(user, providerFor(providerKey));
  return credentialJson(result, providerKey);
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
