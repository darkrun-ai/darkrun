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
  signInWithRedirect,
  getRedirectResult,
  linkWithRedirect,
  GithubAuthProvider,
  OAuthProvider,
} from "https://www.gstatic.com/firebasejs/11.1.0/firebase-auth.js";

const firebaseConfig = {
  apiKey: "AIzaSyDhYi2DQAkbancuR71x3tqQhQ9AE3U29d8",
  // authDomain MUST be the app's own origin (app.darkrun.ai), not the default
  // darkrun.firebaseapp.com. The app signs in via signInWithRedirect (see below),
  // and getRedirectResult on return reads the outcome from the authDomain's
  // storage; when authDomain is a DIFFERENT origin than the app, Chrome's storage
  // partitioning hides that state and the result is lost. app.darkrun.ai is a
  // Firebase Hosting site for this project, so it serves /__/auth/handler natively
  // → same-origin → the redirect round-trip works. (Requires app.darkrun.ai's
  // /__/auth/handler in each OAuth provider's callback/redirect list, alongside
  // the firebaseapp.com one.)
  authDomain: "app.darkrun.ai",
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

// Start a full-page redirect sign-in for `providerKey` ("github" | "gitlab").
// The page navigates to the provider and returns to THIS url; the outcome is
// picked up by consumeRedirect() on the next load.
//
// We use redirect, NOT popup: the app fires sign-in from an async task (a Dioxus
// spawn), so signInWithPopup's window.open lands outside the user gesture and the
// browser blocks it (auth/popup-blocked). A redirect is a top-level navigation,
// always allowed, no popup.
//
// CRITICAL: run signInWithRedirect in a fresh macrotask via setTimeout(0), NOT
// synchronously here. Invoked synchronously inside the wasm-bindgen call frame,
// signInWithRedirect's internal chain stalls before it navigates (verified
// in-browser: no navigation, no error, no network). Deferred to a clean macrotask
// (outside the wasm call stack) it navigates normally — matching a direct
// page-context call, which works. Do not await it either (the page unloads).
export async function startSignInRedirect(providerKey) {
  const provider = providerFor(providerKey);
  setTimeout(() => {
    signInWithRedirect(auth, provider).catch((e) => {
      console.error("darkrun: sign-in redirect failed to start:", e);
    });
  }, 0);
}

// Start a full-page redirect to LINK `providerKey` to the currently signed-in
// account, so ONE darkrun account spans both GitHub and GitLab. Same return path
// as startSignInRedirect; consumeRedirect() reports it with mode "link". Same
// fire-and-forget rule: do NOT await linkWithRedirect (see startSignInRedirect).
export async function startLinkRedirect(providerKey) {
  const user = auth.currentUser;
  if (!user) throw new Error("Sign in before linking another account.");
  const provider = providerFor(providerKey);
  setTimeout(() => {
    linkWithRedirect(user, provider).catch((e) => {
      console.error("darkrun: link redirect failed to start:", e);
    });
  }, 0);
}

// On app load, consume a pending redirect result (present only right after we
// return from a provider). Resolves to JSON:
//   { mode: "signIn" | "link", idToken, accessToken, provider }
// or "" when there is no pending redirect. The dashboard needs the provider OAuth
// access token (to list repos through darkrun-web's /api/repos proxy), which
// Firebase vends only here, on the sign-in/link result's credential.
export async function consumeRedirect() {
  let result;
  try {
    result = await getRedirectResult(auth);
  } catch (e) {
    const code = (e && e.code) || "";
    if (code === "auth/account-exists-with-different-credential") {
      throw new Error(
        "That email is already on darkrun through the other provider. " +
          "Sign in with that one, then add this provider from your dashboard.",
      );
    }
    throw new Error((e && e.message) || "Sign-in didn't complete.");
  }
  if (!result) return "";
  const providerKey = providerKeyOf(result);
  const idToken = await result.user.getIdToken();
  const credential =
    providerKey === "gitlab"
      ? OAuthProvider.credentialFromResult(result)
      : GithubAuthProvider.credentialFromResult(result);
  const accessToken = (credential && credential.accessToken) || "";
  const mode = result.operationType === "link" ? "link" : "signIn";
  return JSON.stringify({ mode, idToken, accessToken, provider: providerKey });
}

// Determine "github" | "gitlab" from a redirect UserCredential.
function providerKeyOf(result) {
  const pid = ((result && result.providerId) || "").toLowerCase();
  if (pid.includes("gitlab")) return "gitlab";
  if (pid.includes("github")) return "github";
  const data = (result.user && result.user.providerData) || [];
  return data.some((p) => (p.providerId || "").includes("gitlab")) ? "gitlab" : "github";
}
