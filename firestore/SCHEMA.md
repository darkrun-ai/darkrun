# darkrun cloud — session registry & remote-access architecture

**No state syncing.** Run state is never copied into a database. The cloud's job
is to let a remote client *reach* a live run, or fall back to git when nothing is
live — not to mirror the run.

```
                         Firebase (auth + registry + signaling + FCM)
                          ▲           ▲                     │ FCM "a gate needs you"
              register +  │           │ discover + signal   ▼
   ┌──────── heartbeat ───┘           └──────────────┐   web / mobile client
   │  HOST (agent MCP session)                       │        │
   │  serves the live run over in-process HTTP/WS  ◀─┼─ tunnel ┘  (reads live, just
   │  (same surface the desktop reads on loopback)   │            like the desktop;
   └─────────────────────────────────────────────────┘            can drive tools)

   No live session?  client reads the committed .darkrun/ state from the GIT repo
   (read-only).
```

## The two access modes

| | Source of truth | Client can | How |
|---|---|---|---|
| **Session active** | the host's live MCP session | **read + write** — watch the run, control the tick, interact, leave feedback | a **tunnel** to the host's HTTP/WS, pushing tools straight onto the host (the same surface the desktop uses on loopback) |
| **No session** | the **git repo** | **read only** | clone/fetch the run's committed `.darkrun/<slug>/` state |

This is the seam for later **channel support** — multiple parties attached to one
live session.

## What Firebase holds (NOT run state)

```
/accounts/{accountId}                    accountId == Firebase Auth uid
    displayName, createdAt
  /identities/{identityId}               provider: "github" | "gitlab"  (linked to one account)
      providerUserId, handle, linkedAt
  /devices/{deviceId}                    fcmToken, platform  (push targets, client-writable)

/sessions/{sessionId}                    a LIVE agent MCP session a host registered
    ownerAccountId                       authz key
    runSlug, title, repoUrl, hostLabel
    alive, lastHeartbeatAt               kept alive WHILE THE HOST IS ALIVE (TTL on stale)
    reach                                how a client opens the tunnel (relay endpoint /
                                           rendezvous id — transport-agnostic)
    needsOperator                        {kind, station} | null  — drives the FCM nudge,
                                           a HINT only (the data is read through the tunnel)
  /signals/{signalId}                    tunnel handshake between host and client
      from, to, payload, createdAt         (e.g. WebRTC offer/answer/ICE, or relay tokens)
```

- The **session doc** is liveness + reachability, nothing more. When it goes
  stale (no heartbeat), it's dead and the run is git-read-only again.
- `needsOperator` + FCM is a *nudge* — "open the app, a gate is waiting"; the
  actual prompt is read live through the tunnel, not from Firestore.
- `/signals` is the rendezvous for establishing the tunnel; the transport
  (WebRTC data channel, a relay, etc.) is pluggable and decided when the tunnel
  is built.

## Who writes what (see `firestore.rules`)

| Path | Host (the user's machine, signed in) | Client (app) |
|---|---|---|
| `accounts/*`, `*/identities/*` | — (admin/linking) | read own |
| `accounts/*/devices/*` | read | **read/write own** (register FCM token) |
| `sessions/{id}` | **create / heartbeat / tear down own** | read own |
| `sessions/{id}/signals/*` | **read/write** (own session) | **read/write** (own session) |

The host runs on the user's machine signed in as that account, so it writes its
OWN session records (no service-account/Admin SDK needed for the registry).
Account/identity linking is the one admin-brokered path.

## Auth & identity

Firebase Auth; the `uid` is the account id. **Multi-identity**: GitHub (built-in)
+ GitLab (generic **OIDC**, Blaze plan), both linked to one `uid` — so a session
is reachable regardless of which host the repo lives on.

## Hosting (app.darkrun.ai)

`firebase.json` serves the Dioxus **web build** (`web/app/dist`) as the
`darkrun-app` site — the fallback surface + universal-link target. The
`/.well-known/apple-app-site-association` + `/assetlinks.json` (from
`web/well-known/`) are copied into the hosting public dir at build and served as
`application/json` (headers in `firebase.json`).

## Setup (one-time, on the operator)

1. Create the Firebase project; set its id in `.firebaserc`.
2. Enable Auth providers: GitHub, and GitLab as a generic OIDC provider.
3. `firebase deploy --only firestore:rules,firestore:indexes`.
