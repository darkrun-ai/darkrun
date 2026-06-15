# darkrun cloud — Firestore data model & sync architecture

The **Firebase-native** backend for accounts, cross-device session sync, and
notifications. There is **no own sync server**: the Rust engine writes run state
to Firestore via a service account, the web/mobile apps subscribe to Firestore
directly for real-time updates, Firebase Auth handles login + multi-identity, and
FCM delivers push.

```
  engine (Rust, Admin SDK) ──writes──▶  Firestore  ◀──subscribes── web / mobile apps
                                          ▲                              │
                                          │ security rules = authz       │ answer a prompt
                                          └──────────────────────────────┘
        Firebase Auth: GitHub (built-in) + GitLab (OIDC), account linking
        FCM: push to an account's registered devices
```

## Auth & identity

- A user signs in with **Firebase Auth**. The Firebase **`uid` is the account
  id** — the partition key for everything below.
- **Multi-identity**: GitHub is a built-in Firebase Auth provider; **GitLab is a
  generic OIDC provider** (requires the Blaze plan). A user *links* both to the
  same `uid` via Firebase account linking, so one account owns runs regardless of
  which host the repo lives on (mirrors darkrun's gh/glab agnosticism).

## Collections

```
/accounts/{accountId}                      accountId == Firebase Auth uid
    displayName, createdAt, updatedAt
  /identities/{identityId}                 one per linked provider
      provider: "github" | "gitlab"
      providerUserId, handle, linkedAt
  /devices/{deviceId}                      push targets (client-writable)
      fcmToken, platform: "ios"|"android"|"web"|"macos", updatedAt

/runs/{runId}                              the live run surface
    ownerAccountId                         == account uid (authz key)
    slug, title, factory, mode, size, status
    repo, branch
    activeStation, phase
    createdAt, updatedAt
  /units/{unitId}                          mirror of .darkrun/<slug>/units/*
      slug, station, status, dependsOn, …
  /sessions/{sessionId}                    interactive prompts (review/question/
      kind, payload, open, raisedAt          direction/picker)
      answer, answeredAt                   ← the ONLY client-writable fields
```

These mirror the engine's on-disk `.darkrun/<slug>/` state (see
`darkrun-core` `StateStore`). The filesystem stays the source of truth on the
machine running the engine; Firestore is the **synced projection** the remote
devices read.

## Who writes what (see `firestore.rules`)

| Path | Engine (Admin SDK) | Client (app) |
|---|---|---|
| `accounts/*` | create/update | read own |
| `accounts/*/identities/*` | link/unlink | read own |
| `accounts/*/devices/*` | read | **read/write own** (register FCM token) |
| `runs/*`, `runs/*/units/*` | full | read own |
| `runs/*/sessions/*` | full | read own; **write only `answer`/`answeredAt`** |

The Admin SDK bypasses rules, so engine writes are unconditional; the rules only
constrain the clients. Answering a prompt is a narrow client write the engine
watches for and acts on — the cross-device equivalent of the desktop's local
answer path.

## Engine → Firestore mirror (next slice)

A `darkrun` engine module (the `firestore` / firestore-rs crate + a service
account JSON via `GOOGLE_APPLICATION_CREDENTIALS`) mirrors run/unit/session
writes to Firestore, gated on config — when no Firebase project is configured the
engine runs exactly as today (local-only). This hooks the same write points the
in-memory `SessionRegistry` live-mirror already uses, so a remote device sees the
same updates a connected desktop does.

## Hosting (app.darkrun.ai)

`firebase.json` serves the Dioxus **web build** (`web/app/dist`) as the
`darkrun-app` hosting site — the fallback surface and the universal-link target.
The `/.well-known/apple-app-site-association` + `/assetlinks.json` (from
`web/well-known/`) must be copied into the hosting `public` dir at build so
app.darkrun.ai serves them with `application/json` (headers set in
`firebase.json`).

## Setup (one-time, on the operator)

1. Create the Firebase project; set its id in `.firebaserc`.
2. Enable Auth providers: GitHub, and GitLab as a generic OIDC provider.
3. `firebase deploy --only firestore:rules,firestore:indexes`.
4. Mint a service-account key for the engine (`GOOGLE_APPLICATION_CREDENTIALS`).
