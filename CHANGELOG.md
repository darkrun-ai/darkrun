# Changelog

All notable changes to darkrun are recorded here. Versions follow semver.

## [0.8.0](https://github.com/darkrun-ai/darkrun/compare/v0.7.0...v0.8.0) (2026-07-02)


### Features

* **api:** add PickerKind::Size ([a5c9e9c](https://github.com/darkrun-ai/darkrun/commit/a5c9e9c8d0f1d16291bf123ccfd521ded8efacd6))
* **api:** local-first connect selection — detect local vs remote ([7a6d04c](https://github.com/darkrun-ai/darkrun/commit/7a6d04c4b8f02cb910e2ffc8f66182a26f546fae))
* **api:** shared tunnel protocol — the durable, local/remote contract ([d3a00e2](https://github.com/darkrun-ai/darkrun/commit/d3a00e2ec02efe4368f0ec7a230295fc220cb594))
* **app:** set the darkrun app icon in the bundle (ships via fastlane) ([79db09d](https://github.com/darkrun-ai/darkrun/commit/79db09d4f40e0096d53b5d56ec41b03e96ecd12d))
* **app:** web push registration — the client half of notify-on-tick ([72a5365](https://github.com/darkrun-ai/darkrun/commit/72a5365b259f80462946ecacb2fd16dfbe3bbd24))
* **app:** wire the Firebase web config (login now functional) ([2af84d6](https://github.com/darkrun-ai/darkrun/commit/2af84d6ba4f3dc5b88c77b95dee9da5fbbce48d4))
* **app:** wire the Web Push VAPID key — remote push fully configured ([9d0750c](https://github.com/darkrun-ai/darkrun/commit/9d0750caa4920646d2d984aa3dbe4eae9470d4f6))
* **cli:** darkrun login — enable remote access (closes the engine-side loop) ([39d99b5](https://github.com/darkrun-ai/darkrun/commit/39d99b51399e32ec4f77c692559a36d4da6b2c34))
* **cli:** engine-data write guard redirects each artifact to its own tool ([bf6f206](https://github.com/darkrun-ai/darkrun/commit/bf6f206d10745d74f7e632ebbb5150715b9c3c39))
* **cloud:** Firebase-native foundation — Firestore data model, rules, project config ([b97cfa4](https://github.com/darkrun-ai/darkrun/commit/b97cfa4f9b09bc2feea7e709003964dd5c1b64d0))
* **core+mcp:** fan the SHA write-guard out to units and briefs ([b4fea39](https://github.com/darkrun-ai/darkrun/commit/b4fea3941827351875f6299a28e0b0232e6a6e72))
* **core+mcp:** SHA optimistic-concurrency guard on artifact writes (knowledge first) ([138fb1e](https://github.com/darkrun-ai/darkrun/commit/138fb1e3aaaffd2cba9a1dc54e0f8e6436a4991f))
* **deeplinks:** add iOS associated-domains entitlement (capability now enabled) ([0b4cb23](https://github.com/darkrun-ai/darkrun/commit/0b4cb23d72e57c67be4a98b39fabfe2ae63e75f0))
* **deeplinks:** fill the AASA appID + wire associated-domains (macOS) ([0df6bc4](https://github.com/darkrun-ai/darkrun/commit/0df6bc45b3ecdc0464bd333d6edf05ee28487878))
* **desktop:** live per-tick session mirror ([a9f565d](https://github.com/darkrun-ai/darkrun/commit/a9f565d50819a71fb1d0546bab096ff24fbb7ccf))
* **desktop:** live per-tick session mirror ([#46](https://github.com/darkrun-ai/darkrun/issues/46)) ([ac76fe0](https://github.com/darkrun-ai/darkrun/commit/ac76fe085336010beaca96fc4258f94fe2741e6e))
* **desktop:** readable question prompts — markdown, text-only cards, real mockups ([d71321a](https://github.com/darkrun-ai/darkrun/commit/d71321afb997617ca66f1797d40a5b8cbf751b9d))
* **engine+desktop:** questions surface on the run + persist across restarts ([598d98e](https://github.com/darkrun-ai/darkrun/commit/598d98ef384116869b23af9653be6c15df01ad6b))
* **engine+desktop:** sessions materialize on demand; chrome is not selectable ([c3c2395](https://github.com/darkrun-ai/darkrun/commit/c3c2395d87f5820142f6cf706e33e59a7800031a))
* **engine:** composite runs — multi-factory topology with sync points ([adcfef6](https://github.com/darkrun-ai/darkrun/commit/adcfef68e6c9bf7aeef5dc6022835ef59573f50d))
* **engine:** engine-driven run-setup elicitation (factory/mode/size pickers) ([905e8ca](https://github.com/darkrun-ai/darkrun/commit/905e8cadb65f1b41d1ca1f751711c350a5171b4b))
* **engine:** mode-gate questions + scope interactive sessions per station ([e990bd4](https://github.com/darkrun-ai/darkrun/commit/e990bd4b8652d384ec6585bea146e603fc594f03))
* **engine:** pull fable from the model selector (Anthropic removed support) ([38a4c80](https://github.com/darkrun-ai/darkrun/commit/38a4c805fe034cce808e1adb455e67b1878cb701))
* **engine:** reject-escalation up the model ladder ([4b7ce33](https://github.com/darkrun-ai/darkrun/commit/4b7ce33cbf4ee9cc80d4b09adc1a1b4bf7a6b31a))
* **engine:** reject-escalation up the model ladder ([#49](https://github.com/darkrun-ai/darkrun/issues/49)) ([b63792b](https://github.com/darkrun-ai/darkrun/commit/b63792bda5c2c6fa477d9c5787b0e4a56c2ee60c))
* **engine:** save_wip clean-tree gate + unit-scope enforcement at completion ([603c106](https://github.com/darkrun-ai/darkrun/commit/603c1069e2e47241049ac71a184c0a4478a185c2))
* **engine:** session-event stream + OTLP telemetry export ([17873e5](https://github.com/darkrun-ai/darkrun/commit/17873e5c66eab59f5b00c89b4eb409919473e661))
* **engine:** spawn the host connector + advertise reachability ([fab84f3](https://github.com/darkrun-ai/darkrun/commit/fab84f3fef3c88ec210fd6ead4868952682e7526))
* **engine:** station drop — the keep-or-drop offer at arrival ([e55f7fb](https://github.com/darkrun-ai/darkrun/commit/e55f7fb008c73455506eb85685894e0cee264876))
* **engine:** the desktop surfaces with the work, not at the first gate ([299011a](https://github.com/darkrun-ai/darkrun/commit/299011a3e69fbb6486f53435a95d8eb2df86eeea))
* **factory:** runtime-verifier run reviewer (the predecessor's strongest gate) ([08d8e45](https://github.com/darkrun-ai/darkrun/commit/08d8e45b0775e60ce7aae045bab3cfe73d4e3602))
* **gates:** await_decision primitive + SessionPayload::resolved() ([eadcea7](https://github.com/darkrun-ai/darkrun/commit/eadcea7f2b71cdfbcef55fde236c9f5b1ccce285))
* **git:** complete Phase 1 gix reads — ls_tree, unresolved_paths, list_worktrees ([b33e131](https://github.com/darkrun-ai/darkrun/commit/b33e1318ec367af7571dff1536c4d81cd4b8a434))
* **git:** gix add_paths + checkout_paths — Phase 2 complete ([47ff4b3](https://github.com/darkrun-ai/darkrun/commit/47ff4b364f32ecd0344704c1861f5f17f0c9bcf2))
* **git:** gix create_branch (Phase 2 start) — idempotent ref write ([b5d7267](https://github.com/darkrun-ai/darkrun/commit/b5d7267bfb6fca912b6e6230e2b639c113661cc6))
* **git:** gix engine-protected three-way merge (Phase 4 — the core safety net) ([5346434](https://github.com/darkrun-ai/darkrun/commit/5346434f2807bd7d6ed4c189914e0868a17b380e))
* **git:** gix linked-worktree create/remove (Phase 3 — first gitoxide gap) ([233a16b](https://github.com/darkrun-ai/darkrun/commit/233a16bbae8deb584dc49f30138d7a3fbec6ec64))
* **git:** gix native fetch (Phase 5) — pure-Rust transport, C-free ([6955549](https://github.com/darkrun-ai/darkrun/commit/69555491cb7cec5f78d6db39809cbdbc0dca62c2))
* **git:** gix reads — is_ancestor, refs_have_identical_trees, merge_in_progress ([c074ed0](https://github.com/darkrun-ai/darkrun/commit/c074ed05688954c32044741d1da3f832e717c4d7))
* **git:** hand-rolled write-tree + gix commit (fork-A internals) ([a7e01b6](https://github.com/darkrun-ai/darkrun/commit/a7e01b6dd23af36a49a9ff22f791803a8981c435))
* **git:** scaffold pure-Rust gitoxide backend (Phase 1 foundation) ([885462d](https://github.com/darkrun-ai/darkrun/commit/885462dd8be770255219ee5193bef7720c27a206))
* **hosting:** run-level draft PR with ready-at-seal flip + compare-URL fallback ([08d8e45](https://github.com/darkrun-ai/darkrun/commit/08d8e45b0775e60ce7aae045bab3cfe73d4e3602))
* **ios:** MATCH_NUKE — one-command cert-slot reset ([bb82760](https://github.com/darkrun-ai/darkrun/commit/bb82760f26c166464c29943e9b10bdd979229074))
* **ios:** RC build numbering for TestFlight (increment until release is cut) ([d49a487](https://github.com/darkrun-ai/darkrun/commit/d49a487326b57ede3e329cae200bf2066250bc44))
* **ios:** safe-area edge-to-edge (toolbar + drawer) + "Darkrun AI" name ([791ceab](https://github.com/darkrun-ai/darkrun/commit/791ceab21dea84a9ee7fed94129124212fbc988d))
* **ios:** universal app (iPhone/iPad/Mac) with mobile content mode ([0db7104](https://github.com/darkrun-ai/darkrun/commit/0db71044193f8cf7ba5c8dc621397a628037182c))
* **login:** relay-token broker carries a browser-minted token to the engine ([b7e2082](https://github.com/darkrun-ai/darkrun/commit/b7e2082b25eb552c4f91fe4d3694c0e78831fbd0))
* **mas:** Phase 1 — shared data root for the Mac App Store app group ([36b51c5](https://github.com/darkrun-ai/darkrun/commit/36b51c56e40ececca5faebf8298f72c72383d723))
* **mas:** Phase 2 — App Sandbox entitlements + macOS .pkg packaging ([2a82f53](https://github.com/darkrun-ai/darkrun/commit/2a82f53654731329cd410b146a9f0acd18abf27a))
* **mcp:** fall back to the app.darkrun.ai deep link when no desktop can open ([29b7830](https://github.com/darkrun-ai/darkrun/commit/29b783020b40d9351b37d33320308a10ffc25993))
* **mcp:** live-mirror the run payload after every mutating tool ([7542bfe](https://github.com/darkrun-ai/darkrun/commit/7542bfeb181ddebb3bd17aa80243f812524471c0))
* **mcp:** local OS notification when a run reaches a gate ([8c15fdf](https://github.com/darkrun-ai/darkrun/commit/8c15fdf15e8cc80e3ed74f403ef32c4535a7f4be))
* **providers:** behavior contracts spliced into prompts + schema-validated settings ([99f2687](https://github.com/darkrun-ai/darkrun/commit/99f26873b330f51073d6ac25c35c5989fdf87da6))
* **relay:** remote-push spine — FCM fan-out + device registration ([d3f15c3](https://github.com/darkrun-ai/darkrun/commit/d3f15c33c6da33f25144d8500babb77a1db0ef7e))
* **site:** 'Open app' link in the header ([#185](https://github.com/darkrun-ai/darkrun/issues/185)) ([8c16e14](https://github.com/darkrun-ai/darkrun/commit/8c16e145ba61b27ba32f3d03acec29adefe6fa7c))
* **site+desktop:** refreshed desktop screenshots + the harness that makes them reproducible ([7df1a90](https://github.com/darkrun-ai/darkrun/commit/7df1a90642ed6604aafd612242bfc311dde9b5a8))
* **site:** add 'Open app' link to the header nav ([9cfbf4a](https://github.com/darkrun-ai/darkrun/commit/9cfbf4a89d5495af7a4d67f52a63156035985ff2))
* **site:** Claude Code's boxed session-start banner on the statusline demo ([5963c7c](https://github.com/darkrun-ai/darkrun/commit/5963c7c89fd95d6b2d7c3a7dc24805c1dddce74c))
* **site:** docs search + JSON-LD structured data ([3e0ceaa](https://github.com/darkrun-ai/darkrun/commit/3e0ceaa93691d2bda46a6bbd0b44df8a81f5c1ea))
* **site:** left/right stepper on the statusline demo ([d9c1c07](https://github.com/darkrun-ai/darkrun/commit/d9c1c074ddb331a7c8cf9fbe874e615ab763af40))
* **site:** left/right stepper on the statusline demo ([#50](https://github.com/darkrun-ai/darkrun/issues/50)) ([27dcedf](https://github.com/darkrun-ai/darkrun/commit/27dcedf7314f1228418af0734b75222ea4319f04))
* **site:** render the statusline demo in situ, under Claude Code's prompt box ([6c42eab](https://github.com/darkrun-ai/darkrun/commit/6c42eab09a1f759b7ed58c46915b69e58107a3d0))
* **site:** social card (Open Graph / Twitter) — the factory-line hero ([a3a2781](https://github.com/darkrun-ai/darkrun/commit/a3a2781765bd6cec1d8e614e6eda034bcd7ccb63))
* **site:** the terminal panels follow the site theme ([4752490](https://github.com/darkrun-ai/darkrun/commit/47524906b4debf190be78ce582589ab82683ed37))
* **statusline+site:** phase-track pips + the status line on the website + the fable tier ([5a7ae9a](https://github.com/darkrun-ai/darkrun/commit/5a7ae9ac7af4421911b2c84a97f80bcb0da48286))
* **statusline:** explorer chips at Spec + dev launcher freshness ([170ea16](https://github.com/darkrun-ai/darkrun/commit/170ea16fa08ddf17ae2b9b13c55dfbcbe31f724f))
* **tunnel:** host connector — durably bridge the relay to the local engine ([4065891](https://github.com/darkrun-ai/darkrun/commit/4065891ed6bf7ee0f5c48dd1f4a7d678e0508fb0))
* **tunnel:** host connector pushes a remote notification on gate entry ([e294a91](https://github.com/darkrun-ai/darkrun/commit/e294a91e8a2c0ed6afc59059e5aa6d0c14848b27))
* **ui:** shared Sidebar component ([#184](https://github.com/darkrun-ai/darkrun/issues/184)) ([b0cb04a](https://github.com/darkrun-ai/darkrun/commit/b0cb04a9d9898a2989cd0c16712fccd18d6458a6))
* **ui:** shared Sidebar component (runs-by-project, actions, identity) ([ee7e44b](https://github.com/darkrun-ai/darkrun/commit/ee7e44b3aecdad4549d86f1503a4b051475d923b))
* **web:** actionable gate + station narrative in the web app ([bb6c7a7](https://github.com/darkrun-ai/darkrun/commit/bb6c7a748425e8481185bdc6891e446a453279dc))
* **web:** app.darkrun.ai — the Dioxus web client for live remote runs ([694e4a4](https://github.com/darkrun-ai/darkrun/commit/694e4a4820b47d43253c63151f656bead2cd4991))
* **web:** client-addressed relay routing — read into a live session on connect ([48c1ae5](https://github.com/darkrun-ai/darkrun/commit/48c1ae56eac2f0badb22f606cf5f348cba0e7737))
* **web:** Firebase ID-token verifier secures the relay + /darkrun:darkrun-login ([23ea0f1](https://github.com/darkrun-ai/darkrun/commit/23ea0f15e9af7231192316b208f61115dd473e48))
* **web:** Firestore-backed device registry — push survives restarts ([5411693](https://github.com/darkrun-ai/darkrun/commit/5411693ebf54408fe73696906ac05c66180fc4f2))
* **web:** Font Awesome brand icons for sign-in + app iconography ([#183](https://github.com/darkrun-ai/darkrun/issues/183)) ([45ce997](https://github.com/darkrun-ai/darkrun/commit/45ce99781aec3e9e300baea0e1d7591385fc8e15))
* **web:** Font Awesome icons — brand icons for sign-in + app iconography ([2ebcbd2](https://github.com/darkrun-ai/darkrun/commit/2ebcbd26cfa13bc0d59bda8892b957aef7c51452))
* **web:** GitHub + GitLab on one account + combined portfolio ([#173](https://github.com/darkrun-ai/darkrun/issues/173)) ([80ac32a](https://github.com/darkrun-ai/darkrun/commit/80ac32a0e5dbd9a226b0fac6e8069cd3c2bc27d6))
* **web:** Google service-account token source — FCM push goes live ([19b60cc](https://github.com/darkrun-ai/darkrun/commit/19b60ccd10d7822a8d352da07a9111a75a3291bb))
* **web:** link GitHub + GitLab to one account + combined portfolio ([e07aa38](https://github.com/darkrun-ai/darkrun/commit/e07aa38155c9f61a5963e7cf124ee7b790eb0205))
* **web:** remote-tunnel relay — reverse-WS bridge in darkrun-web ([36e512b](https://github.com/darkrun-ai/darkrun/commit/36e512b43e8175d9dce3883da70292684cf63f2a))
* **web:** web app Firebase Auth sign-in — closes the login chain ([11abc83](https://github.com/darkrun-ai/darkrun/commit/11abc83ddfea1bfbe6c99c53db7baa105ef1d121))


### Bug Fixes

* **api:** make openapi.json a fixed point of release-please's rewrite ([f3bbf95](https://github.com/darkrun-ai/darkrun/commit/f3bbf957a4ae51d0746dfed79006678250a596e0))
* **api:** make openapi.json a fixed point of release-please's rewrite ([#42](https://github.com/darkrun-ai/darkrun/issues/42)) ([c964465](https://github.com/darkrun-ai/darkrun/commit/c96446584c9e1c5f84953ec92df57a909e4999a1))
* **app:** defer signInWithRedirect out of the wasm call frame + no-cache assets ([#190](https://github.com/darkrun-ai/darkrun/issues/190)) ([82fe605](https://github.com/darkrun-ai/darkrun/commit/82fe60544606972760bc8a8b60c39d08d246c3b4))
* **app:** defer signInWithRedirect to a macrotask; no-cache all assets ([04c6016](https://github.com/darkrun-ai/darkrun/commit/04c601607a45ded10b0f31197cae56a4b9a748ea))
* **app:** fire-and-forget sign-in redirect + no-cache shell + messaging authDomain ([#189](https://github.com/darkrun-ai/darkrun/issues/189)) ([9b95c4a](https://github.com/darkrun-ai/darkrun/commit/9b95c4af67aa2ee643f40c4bdecaff813e6f32cf))
* **app:** fire-and-forget the sign-in redirect (stop the hang) ([5695a06](https://github.com/darkrun-ai/darkrun/commit/5695a064985e6cff70d127ba4714fa2326aa3c31))
* **app:** Firebase authDomain = app.darkrun.ai (unhangs sign-in) ([6acb2ed](https://github.com/darkrun-ai/darkrun/commit/6acb2ed311c99200cb902e2ec051d8268e364284))
* **app:** mount the shared theme — kills the white viewport border ([0614437](https://github.com/darkrun-ai/darkrun/commit/0614437c98c24fcd2267626d80ae64bd0978479d))
* **app:** same-origin Firebase authDomain (unhangs GitHub + GitLab sign-in) ([#187](https://github.com/darkrun-ai/darkrun/issues/187)) ([02873d1](https://github.com/darkrun-ai/darkrun/commit/02873d1f16623c4bf3d0e2c73bc0d5099baf25b6))
* **app:** sign in via full-page redirect, not popup ([2f91768](https://github.com/darkrun-ai/darkrun/commit/2f917682d62a41f657aea274bc32a64e3807bd4b))
* **app:** sign in via redirect, not popup (fixes the hang) ([#188](https://github.com/darkrun-ai/darkrun/issues/188)) ([dbaa3d2](https://github.com/darkrun-ai/darkrun/commit/dbaa3d22c2699f103e885a00dd299f8726280802))
* **app:** sign-in can't hang forever + friendly auth errors ([#186](https://github.com/darkrun-ai/darkrun/issues/186)) ([0dd57e8](https://github.com/darkrun-ai/darkrun/commit/0dd57e8206f94cf237abf777202869fa2f8b3704))
* **app:** spawn sign-in from a persistent scope (the actual hang) ([#192](https://github.com/darkrun-ai/darkrun/issues/192)) ([b555379](https://github.com/darkrun-ai/darkrun/commit/b555379930665e0034f52f4dd26378e05360a03b))
* **app:** spawn sign-in from a persistent scope (the real hang) ([67b9c17](https://github.com/darkrun-ai/darkrun/commit/67b9c17b8674b9aa9b01c38126e6d763bf364a8e))
* **app:** stop sign-in hanging forever; friendly auth errors ([47a06b7](https://github.com/darkrun-ai/darkrun/commit/47a06b7c9cde98e200612d880a5a0895041cd811))
* clear all code-scanning alerts (clippy + CodeQL) ([a82d211](https://github.com/darkrun-ai/darkrun/commit/a82d211433c5256c9dbfcaa77dc6dd0d71fb7f77))
* **cloud:** correct the Firebase model — session registry, NOT a state mirror ([52b2fbe](https://github.com/darkrun-ai/darkrun/commit/52b2fbe00e9c0053a5ee001b5922f641b6ca4ecb))
* **desktop:** a stale dev launch bundle execs the fresher build ([042f83b](https://github.com/darkrun-ai/darkrun/commit/042f83be6a509fed0bdd46383f0b18084f0b812d))
* **desktop:** drag the window by the toolbar (wry ignores -webkit-app-region) ([f96b771](https://github.com/darkrun-ai/darkrun/commit/f96b7717ad4272e7187374344fbf2c36bf9a041a))
* **desktop:** key sidebar run lists by project slug, not display name ([14ec652](https://github.com/darkrun-ai/darkrun/commit/14ec652f6231531a49ce03134547cbdeeec64f11))
* **desktop:** let the welcome/projects surface fill the main pane ([920c2e0](https://github.com/darkrun-ai/darkrun/commit/920c2e096edf085e877f935ce9d1c423f3b06ae2))
* **desktop:** project identity self-heal, --worktree launch, choosable clone path ([c0efbbf](https://github.com/darkrun-ai/darkrun/commit/c0efbbf82d718677187fde831bf26e79a791bcb6))
* **desktop:** welcome/projects surface fills the main pane ([#166](https://github.com/darkrun-ai/darkrun/issues/166)) ([c1f7bda](https://github.com/darkrun-ai/darkrun/commit/c1f7bdaf2c2fb7e36bba4b1b3b6d6c29e4aaddb0))
* **engine:** answering an interactive session dismisses it + surfaces the next ([5828727](https://github.com/darkrun-ai/darkrun/commit/5828727a91a98889a383ec8d207f4f5e806cb434))
* **engine:** raising a question/direction/picker gate launches the desktop ([e5586ef](https://github.com/darkrun-ai/darkrun/commit/e5586efaf3adb0dd01d9c96b6e7c179fae6259e4))
* **git:** normalize the common dir before deriving the project root ([028d2e3](https://github.com/darkrun-ai/darkrun/commit/028d2e38e5574127c22863854da49166df46f4d9))
* **http:** the Mine predicate checks the run's STABLE branch ([ab8eb8f](https://github.com/darkrun-ai/darkrun/commit/ab8eb8ff2dd1b7043183b82d095b327ffea98922))
* **ios:** bootstrap actually sets the secrets (kill the unbound-var abort) ([dba244a](https://github.com/darkrun-ai/darkrun/commit/dba244a11f2292f73148c203bc95b6a3b15ec67c))
* **ios:** bootstrap drops the stale Gemfile.lock (bundler 1.17.2 vs Ruby 3.4) ([df2affa](https://github.com/darkrun-ai/darkrun/commit/df2affaf7bc31af7edcb6258f703401151c311e6))
* **ios:** bootstrap prefers a modern Ruby; drop Appfile placeholders ([9102bc5](https://github.com/darkrun-ai/darkrun/commit/9102bc57bbc77eefd114c4bf51947f5f4050af5a))
* **ios:** bootstrap survives LibreSSL — use real OpenSSL for match ([5f04f92](https://github.com/darkrun-ai/darkrun/commit/5f04f92fd08a63b7ad421edf67eac7cf1d7a1d48))
* **ios:** build for device (not simulator) + compile the app-icon asset catalog ([038e441](https://github.com/darkrun-ai/darkrun/commit/038e4411d3d65d945e93c14c4089d3aa2fe22d08))
* **ios:** build from the dx .app, not a phantom Xcode project ([055a9f5](https://github.com/darkrun-ai/darkrun/commit/055a9f56c660a4571ce395c62bdbbea9a5399e6e))
* **ios:** build with a released (non-beta) Xcode + inject DT* Info.plist keys ([6d64c3e](https://github.com/darkrun-ai/darkrun/commit/6d64c3e40fe8bc3cc493d8f51f9be0a80aaed842))
* **ios:** declare ITSAppUsesNonExemptEncryption=false to stop the export prompt ([a9651ce](https://github.com/darkrun-ai/darkrun/commit/a9651ced610ae8ce0a28c450162f413607a8db98))
* **ios:** disable webpage zoom in the mobile viewport (app feel) ([e1b3e3e](https://github.com/darkrun-ai/darkrun/commit/e1b3e3e8e83f1e4fbed8d48d4138a6bb6dbe2297))
* **ios:** don't apply the desktop window size on mobile (the real layout fix) ([5b770de](https://github.com/darkrun-ai/darkrun/commit/5b770de21ac374bd6116cc0a700234b825d74a5f))
* **ios:** find the match profile in Xcode 26's install location ([d9a211c](https://github.com/darkrun-ai/darkrun/commit/d9a211cacb8d7a82899c435c92ec45ecb09d1bd5))
* **ios:** force device-width viewport so the mobile drawer layout fires ([e45a1d0](https://github.com/darkrun-ai/darkrun/commit/e45a1d005ed4403a4ca562da765b8fd6de6f7588))
* **ios:** own the mobile index with a clean viewport (drawer + safe area) ([40537a5](https://github.com/darkrun-ai/darkrun/commit/40537a52b1a496e3b6cdb6541103a020d67d7dcc))
* **ios:** pass App Store binary validation (Info.plist, symlinks, iOS 26 SDK) ([5a7aa28](https://github.com/darkrun-ai/darkrun/commit/5a7aa287021ff8443666f289b480ecca925633be))
* **ios:** pin IPHONEOS_DEPLOYMENT_TARGET=15.0 for the device build ([05deaf2](https://github.com/darkrun-ai/darkrun/commit/05deaf22ca9efdaa295dd06bb5565e395fac493c))
* **ios:** pin platform: ios so the upload doesn't prompt in CI ([74d6c4a](https://github.com/darkrun-ai/darkrun/commit/74d6c4ab820e78ba0e1d9588a8e377865ea26757))
* **ios:** setup_ci so codesign doesn't hang on a keychain prompt ([8df8501](https://github.com/darkrun-ai/darkrun/commit/8df850169f07230ca9945404e223eb7397b62127))
* **ios:** upload_to_testflight option is app_platform, not platform ([94cf0d5](https://github.com/darkrun-ai/darkrun/commit/94cf0d58524463d4ebbf584ee164b22e28e8f2d8))
* **mas:** add network.server (boot) + name bundle 'Darkrun AI.app' ([be9ddf4](https://github.com/darkrun-ai/darkrun/commit/be9ddf45caed3b6f2bb2fa5ba6e4e3811cef83d4))
* **mas:** don't let identity lookup trip pipefail + dump identities ([28d96bf](https://github.com/darkrun-ai/darkrun/commit/28d96bf5563804c573f1652effb90e7e333edcfc))
* **mas:** find installer identity under basic policy, not codesigning ([0781cfb](https://github.com/darkrun-ai/darkrun/commit/0781cfbe82492e348777a9dea33dc4e90ca610ac))
* **mas:** generate_apple_certs:false for the installer cert ([dd9e641](https://github.com/darkrun-ai/darkrun/commit/dd9e641c201d1ea772753d740c49012c42e3e4e2))
* **mas:** inject application-identifier entitlement (ITMS-90886) ([#164](https://github.com/darkrun-ai/darkrun/issues/164)) ([0fc15c3](https://github.com/darkrun-ai/darkrun/commit/0fc15c3cee38bd90218d4a83cf850f2144f52494))
* **mas:** inject application-identifier entitlement to fix ITMS-90886 ([d13894d](https://github.com/darkrun-ai/darkrun/commit/d13894d85fe3a6e093f7a289b5365cabd810180b))
* **mas:** macOS app boot crash (network.server) + name 'Darkrun AI' ([#165](https://github.com/darkrun-ai/darkrun/issues/165)) ([9d0d4a3](https://github.com/darkrun-ai/darkrun/commit/9d0d4a31ec49d96272a0bb73bbb84b106b4531eb))
* **mas:** target macOS 12.0 so the arm64-only build passes validation ([ee64717](https://github.com/darkrun-ai/darkrun/commit/ee64717bd03a5d6b447777227378b69c10a34e53))
* **mas:** unblock Mac App Store .pkg signing + upload ([#163](https://github.com/darkrun-ai/darkrun/issues/163)) ([f8e8383](https://github.com/darkrun-ai/darkrun/commit/f8e83834b04fe213b15b948d94ca651f0f68af97))
* **mas:** upload .pkg binary only, skip first-version metadata upload ([071b8d4](https://github.com/darkrun-ai/darkrun/commit/071b8d475285f6a5d8ca1ee5e5ac8b2df0d54d59))
* **mcp:** unbreak the Windows release build (notify.rs E0282) ([1106f18](https://github.com/darkrun-ai/darkrun/commit/1106f1878b21be24fad1e15e844557b7755e56d7))
* picker UX (chrome, stale selection, auto-close) + same-commit checkout ([a085a7d](https://github.com/darkrun-ai/darkrun/commit/a085a7d6165b3015963ec23a5ec9e0f460c445b3))
* propagate the 0.2.0 release bump (unblock all open PRs) ([#20](https://github.com/darkrun-ai/darkrun/issues/20)) ([ae45020](https://github.com/darkrun-ai/darkrun/commit/ae4502037ebb67513017416143c6830a2f77489b))
* **release:** point package.json repository at darkrun-ai/darkrun ([#33](https://github.com/darkrun-ai/darkrun/issues/33)) ([0ac2b8f](https://github.com/darkrun-ai/darkrun/commit/0ac2b8fe37f6bd9725fa55068f805a50ecc9f5f1))
* **signing:** force-regenerate App Store profiles in the certs lanes ([cd86b22](https://github.com/darkrun-ai/darkrun/commit/cd86b22da79675a5fb05a67f3ffd6eba32e25d0f))
* **signing:** installer cert has no profile — skip_provisioning_profiles (macos) ([9536371](https://github.com/darkrun-ai/darkrun/commit/95363714c2d53290595f8cbaf25fa2e9dca0a73f))
* **site:** clay banner box, sized to the panel ([50fb85d](https://github.com/darkrun-ai/darkrun/commit/50fb85d60ed5400e1956dd7b6b5e188f52f6ade7))
* **site:** make the feed-date suggestions compile + emit valid formats ([88f4061](https://github.com/darkrun-ai/darkrun/commit/88f40619b36a4df2f03515140385072dba8dc2b6))
* **site:** preview question sample sets run_slug ([68f3edf](https://github.com/darkrun-ai/darkrun/commit/68f3edf99823643f0a259822c2e60548d59e4e0f))
* **site:** serve /assets/* (OG image, favicon, screenshots) ([48cf7a6](https://github.com/darkrun-ai/darkrun/commit/48cf7a6835fab98d7e79ae017e2c47c68fd92092))
* **site:** statusline stepper dots use the shared accent pill; drop the redundant slideshow slide ([eea29bd](https://github.com/darkrun-ai/darkrun/commit/eea29bd6b26f5f4d64a98f511872a9036acd35dc))
* **statusline:** read on light terminals — bold default-fg for slug and passed pips ([73be50f](https://github.com/darkrun-ai/darkrun/commit/73be50f3d28598ea2b1e742aee4e7e4e4aa03dd9))
* **ui:** saturate the tab count pill at 99+ ([8f5588a](https://github.com/darkrun-ai/darkrun/commit/8f5588a6b0310defe1f3421765ed476e5d89d19b))
* **web:** app wordmark + tab title match the website ([#174](https://github.com/darkrun-ai/darkrun/issues/174)) ([fe98d89](https://github.com/darkrun-ai/darkrun/commit/fe98d89504551a03c221bc50977c0e868dad217f))
* **web:** app wordmark matches the website's interactive look ([#175](https://github.com/darkrun-ai/darkrun/issues/175)) ([a4faedb](https://github.com/darkrun-ai/darkrun/commit/a4faedb772b7405217ac32c5c6154086c113e6ce))
* **web:** correct app wordmark + tab title (match the website) ([2526cea](https://github.com/darkrun-ai/darkrun/commit/2526cea2ff3fcd5a1ffb63c945b32a065ae82adb))
* **web:** CORS on /api/repos for the cross-origin dashboard ([#176](https://github.com/darkrun-ai/darkrun/issues/176)) ([77e42f2](https://github.com/darkrun-ai/darkrun/commit/77e42f2a54f93542dcd3287c1b2e002a17fc5093))
* **web:** CORS on /api/repos so the dashboard can call it cross-origin ([d447e69](https://github.com/darkrun-ai/darkrun/commit/d447e694132d8c85d9bf0d62a38faec1c1603d3f))
* **web:** dashboard default + both providers + narrower GitLab scope ([#182](https://github.com/darkrun-ai/darkrun/issues/182)) ([cb19524](https://github.com/darkrun-ai/darkrun/commit/cb195244fb79e96e072eeca9de926c739de888e8))
* **web:** default to the dashboard, show both providers, narrow GitLab scope ([3e7e1c3](https://github.com/darkrun-ai/darkrun/commit/3e7e1c3ee604c66e8fb16b9ae9443f1bfccda9ce))
* **web:** Dioxus.toml [web.resource] needs a dev subtable so dx build works ([5a0944f](https://github.com/darkrun-ai/darkrun/commit/5a0944f8c53bc8cdc62326c5c7171d8c9484eaec))
* **web:** match the website's interactive wordmark in the app ([e5bd8a3](https://github.com/darkrun-ai/darkrun/commit/e5bd8a3e5689bc8530ab9046a037e866bac675a8))
* **web:** pin dark theme so the wordmark shows on app.darkrun.ai ([#180](https://github.com/darkrun-ai/darkrun/issues/180)) ([19e3caf](https://github.com/darkrun-ai/darkrun/commit/19e3cafd2402dc7832cf8f6f6c96e5856ac81af5))
* **web:** pin data-theme=dark so the wordmark renders on the dark app ([fdb26e5](https://github.com/darkrun-ai/darkrun/commit/fdb26e57642795dfcd3d61cfb91460a8982bf2dd))
* **web:** select rust_crypto backend for jsonwebtoken 10 ([4ef9cdb](https://github.com/darkrun-ai/darkrun/commit/4ef9cdbbaea1efe8e2e8c322f2ba6aaba9d0a3d0))

## [0.7.0](https://github.com/darkrun-ai/darkrun/compare/v0.6.0...v0.7.0) (2026-06-17)


### Features

* **app:** web push registration — the client half of notify-on-tick ([72a5365](https://github.com/darkrun-ai/darkrun/commit/72a5365b259f80462946ecacb2fd16dfbe3bbd24))
* **app:** wire the Firebase web config (login now functional) ([2af84d6](https://github.com/darkrun-ai/darkrun/commit/2af84d6ba4f3dc5b88c77b95dee9da5fbbce48d4))
* **app:** wire the Web Push VAPID key — remote push fully configured ([9d0750c](https://github.com/darkrun-ai/darkrun/commit/9d0750caa4920646d2d984aa3dbe4eae9470d4f6))
* **web:** Firestore-backed device registry — push survives restarts ([5411693](https://github.com/darkrun-ai/darkrun/commit/5411693ebf54408fe73696906ac05c66180fc4f2))


### Bug Fixes

* **mcp:** unbreak the Windows release build (notify.rs E0282) ([1106f18](https://github.com/darkrun-ai/darkrun/commit/1106f1878b21be24fad1e15e844557b7755e56d7))

## [0.6.0](https://github.com/darkrun-ai/darkrun/compare/v0.5.0...v0.6.0) (2026-06-16)


### Features

* **relay:** remote-push spine — FCM fan-out + device registration ([d3f15c3](https://github.com/darkrun-ai/darkrun/commit/d3f15c33c6da33f25144d8500babb77a1db0ef7e))
* **tunnel:** host connector pushes a remote notification on gate entry ([e294a91](https://github.com/darkrun-ai/darkrun/commit/e294a91e8a2c0ed6afc59059e5aa6d0c14848b27))
* **web:** Google service-account token source — FCM push goes live ([19b60cc](https://github.com/darkrun-ai/darkrun/commit/19b60ccd10d7822a8d352da07a9111a75a3291bb))


### Bug Fixes

* **app:** mount the shared theme — kills the white viewport border ([0614437](https://github.com/darkrun-ai/darkrun/commit/0614437c98c24fcd2267626d80ae64bd0978479d))
* **site:** serve /assets/* (OG image, favicon, screenshots) ([48cf7a6](https://github.com/darkrun-ai/darkrun/commit/48cf7a6835fab98d7e79ae017e2c47c68fd92092))

## [0.5.0](https://github.com/darkrun-ai/darkrun/compare/v0.4.0...v0.5.0) (2026-06-16)


### Features

* **api:** add PickerKind::Size ([a5c9e9c](https://github.com/darkrun-ai/darkrun/commit/a5c9e9c8d0f1d16291bf123ccfd521ded8efacd6))
* **api:** local-first connect selection — detect local vs remote ([7a6d04c](https://github.com/darkrun-ai/darkrun/commit/7a6d04c4b8f02cb910e2ffc8f66182a26f546fae))
* **api:** shared tunnel protocol — the durable, local/remote contract ([d3a00e2](https://github.com/darkrun-ai/darkrun/commit/d3a00e2ec02efe4368f0ec7a230295fc220cb594))
* **cli:** darkrun login — enable remote access (closes the engine-side loop) ([39d99b5](https://github.com/darkrun-ai/darkrun/commit/39d99b51399e32ec4f77c692559a36d4da6b2c34))
* **cli:** engine-data write guard redirects each artifact to its own tool ([bf6f206](https://github.com/darkrun-ai/darkrun/commit/bf6f206d10745d74f7e632ebbb5150715b9c3c39))
* **cloud:** Firebase-native foundation — Firestore data model, rules, project config ([b97cfa4](https://github.com/darkrun-ai/darkrun/commit/b97cfa4f9b09bc2feea7e709003964dd5c1b64d0))
* **core+mcp:** fan the SHA write-guard out to units and briefs ([b4fea39](https://github.com/darkrun-ai/darkrun/commit/b4fea3941827351875f6299a28e0b0232e6a6e72))
* **core+mcp:** SHA optimistic-concurrency guard on artifact writes (knowledge first) ([138fb1e](https://github.com/darkrun-ai/darkrun/commit/138fb1e3aaaffd2cba9a1dc54e0f8e6436a4991f))
* **desktop:** readable question prompts — markdown, text-only cards, real mockups ([d71321a](https://github.com/darkrun-ai/darkrun/commit/d71321afb997617ca66f1797d40a5b8cbf751b9d))
* **engine+desktop:** questions surface on the run + persist across restarts ([598d98e](https://github.com/darkrun-ai/darkrun/commit/598d98ef384116869b23af9653be6c15df01ad6b))
* **engine+desktop:** sessions materialize on demand; chrome is not selectable ([c3c2395](https://github.com/darkrun-ai/darkrun/commit/c3c2395d87f5820142f6cf706e33e59a7800031a))
* **engine:** engine-driven run-setup elicitation (factory/mode/size pickers) ([905e8ca](https://github.com/darkrun-ai/darkrun/commit/905e8cadb65f1b41d1ca1f751711c350a5171b4b))
* **engine:** mode-gate questions + scope interactive sessions per station ([e990bd4](https://github.com/darkrun-ai/darkrun/commit/e990bd4b8652d384ec6585bea146e603fc594f03))
* **engine:** pull fable from the model selector (Anthropic removed support) ([38a4c80](https://github.com/darkrun-ai/darkrun/commit/38a4c805fe034cce808e1adb455e67b1878cb701))
* **engine:** spawn the host connector + advertise reachability ([fab84f3](https://github.com/darkrun-ai/darkrun/commit/fab84f3fef3c88ec210fd6ead4868952682e7526))
* **engine:** the desktop surfaces with the work, not at the first gate ([299011a](https://github.com/darkrun-ai/darkrun/commit/299011a3e69fbb6486f53435a95d8eb2df86eeea))
* **login:** relay-token broker carries a browser-minted token to the engine ([b7e2082](https://github.com/darkrun-ai/darkrun/commit/b7e2082b25eb552c4f91fe4d3694c0e78831fbd0))
* **mcp:** fall back to the app.darkrun.ai deep link when no desktop can open ([29b7830](https://github.com/darkrun-ai/darkrun/commit/29b783020b40d9351b37d33320308a10ffc25993))
* **mcp:** live-mirror the run payload after every mutating tool ([7542bfe](https://github.com/darkrun-ai/darkrun/commit/7542bfeb181ddebb3bd17aa80243f812524471c0))
* **mcp:** local OS notification when a run reaches a gate ([8c15fdf](https://github.com/darkrun-ai/darkrun/commit/8c15fdf15e8cc80e3ed74f403ef32c4535a7f4be))
* **site:** social card (Open Graph / Twitter) — the factory-line hero ([a3a2781](https://github.com/darkrun-ai/darkrun/commit/a3a2781765bd6cec1d8e614e6eda034bcd7ccb63))
* **statusline:** explorer chips at Spec + dev launcher freshness ([170ea16](https://github.com/darkrun-ai/darkrun/commit/170ea16fa08ddf17ae2b9b13c55dfbcbe31f724f))
* **tunnel:** host connector — durably bridge the relay to the local engine ([4065891](https://github.com/darkrun-ai/darkrun/commit/4065891ed6bf7ee0f5c48dd1f4a7d678e0508fb0))
* **web:** actionable gate + station narrative in the web app ([bb6c7a7](https://github.com/darkrun-ai/darkrun/commit/bb6c7a748425e8481185bdc6891e446a453279dc))
* **web:** app.darkrun.ai — the Dioxus web client for live remote runs ([694e4a4](https://github.com/darkrun-ai/darkrun/commit/694e4a4820b47d43253c63151f656bead2cd4991))
* **web:** client-addressed relay routing — read into a live session on connect ([48c1ae5](https://github.com/darkrun-ai/darkrun/commit/48c1ae56eac2f0badb22f606cf5f348cba0e7737))
* **web:** Firebase ID-token verifier secures the relay + /darkrun:darkrun-login ([23ea0f1](https://github.com/darkrun-ai/darkrun/commit/23ea0f15e9af7231192316b208f61115dd473e48))
* **web:** remote-tunnel relay — reverse-WS bridge in darkrun-web ([36e512b](https://github.com/darkrun-ai/darkrun/commit/36e512b43e8175d9dce3883da70292684cf63f2a))
* **web:** web app Firebase Auth sign-in — closes the login chain ([11abc83](https://github.com/darkrun-ai/darkrun/commit/11abc83ddfea1bfbe6c99c53db7baa105ef1d121))


### Bug Fixes

* **cloud:** correct the Firebase model — session registry, NOT a state mirror ([52b2fbe](https://github.com/darkrun-ai/darkrun/commit/52b2fbe00e9c0053a5ee001b5922f641b6ca4ecb))
* **desktop:** a stale dev launch bundle execs the fresher build ([042f83b](https://github.com/darkrun-ai/darkrun/commit/042f83be6a509fed0bdd46383f0b18084f0b812d))
* **desktop:** key sidebar run lists by project slug, not display name ([14ec652](https://github.com/darkrun-ai/darkrun/commit/14ec652f6231531a49ce03134547cbdeeec64f11))
* **engine:** answering an interactive session dismisses it + surfaces the next ([5828727](https://github.com/darkrun-ai/darkrun/commit/5828727a91a98889a383ec8d207f4f5e806cb434))
* **engine:** raising a question/direction/picker gate launches the desktop ([e5586ef](https://github.com/darkrun-ai/darkrun/commit/e5586efaf3adb0dd01d9c96b6e7c179fae6259e4))
* **git:** normalize the common dir before deriving the project root ([028d2e3](https://github.com/darkrun-ai/darkrun/commit/028d2e38e5574127c22863854da49166df46f4d9))
* **http:** the Mine predicate checks the run's STABLE branch ([ab8eb8f](https://github.com/darkrun-ai/darkrun/commit/ab8eb8ff2dd1b7043183b82d095b327ffea98922))
* picker UX (chrome, stale selection, auto-close) + same-commit checkout ([a085a7d](https://github.com/darkrun-ai/darkrun/commit/a085a7d6165b3015963ec23a5ec9e0f460c445b3))
* **site:** preview question sample sets run_slug ([68f3edf](https://github.com/darkrun-ai/darkrun/commit/68f3edf99823643f0a259822c2e60548d59e4e0f))
* **web:** select rust_crypto backend for jsonwebtoken 10 ([4ef9cdb](https://github.com/darkrun-ai/darkrun/commit/4ef9cdbbaea1efe8e2e8c322f2ba6aaba9d0a3d0))

## [0.4.0](https://github.com/darkrun-ai/darkrun/compare/v0.3.0...v0.4.0) (2026-06-11)


### Features

* **site:** Claude Code's boxed session-start banner on the statusline demo ([5963c7c](https://github.com/darkrun-ai/darkrun/commit/5963c7c89fd95d6b2d7c3a7dc24805c1dddce74c))
* **site:** left/right stepper on the statusline demo ([d9c1c07](https://github.com/darkrun-ai/darkrun/commit/d9c1c074ddb331a7c8cf9fbe874e615ab763af40))
* **site:** left/right stepper on the statusline demo ([#50](https://github.com/darkrun-ai/darkrun/issues/50)) ([27dcedf](https://github.com/darkrun-ai/darkrun/commit/27dcedf7314f1228418af0734b75222ea4319f04))
* **site:** render the statusline demo in situ, under Claude Code's prompt box ([6c42eab](https://github.com/darkrun-ai/darkrun/commit/6c42eab09a1f759b7ed58c46915b69e58107a3d0))
* **site:** the terminal panels follow the site theme ([4752490](https://github.com/darkrun-ai/darkrun/commit/47524906b4debf190be78ce582589ab82683ed37))


### Bug Fixes

* **site:** clay banner box, sized to the panel ([50fb85d](https://github.com/darkrun-ai/darkrun/commit/50fb85d60ed5400e1956dd7b6b5e188f52f6ade7))
* **site:** statusline stepper dots use the shared accent pill; drop the redundant slideshow slide ([eea29bd](https://github.com/darkrun-ai/darkrun/commit/eea29bd6b26f5f4d64a98f511872a9036acd35dc))
* **statusline:** read on light terminals — bold default-fg for slug and passed pips ([73be50f](https://github.com/darkrun-ai/darkrun/commit/73be50f3d28598ea2b1e742aee4e7e4e4aa03dd9))
* **ui:** saturate the tab count pill at 99+ ([8f5588a](https://github.com/darkrun-ai/darkrun/commit/8f5588a6b0310defe1f3421765ed476e5d89d19b))

## [0.3.0](https://github.com/darkrun-ai/darkrun/compare/v0.2.1...v0.3.0) (2026-06-11)


### Features

* **desktop:** live per-tick session mirror ([a9f565d](https://github.com/darkrun-ai/darkrun/commit/a9f565d50819a71fb1d0546bab096ff24fbb7ccf))
* **desktop:** live per-tick session mirror ([#46](https://github.com/darkrun-ai/darkrun/issues/46)) ([ac76fe0](https://github.com/darkrun-ai/darkrun/commit/ac76fe085336010beaca96fc4258f94fe2741e6e))
* **engine:** composite runs — multi-factory topology with sync points ([adcfef6](https://github.com/darkrun-ai/darkrun/commit/adcfef68e6c9bf7aeef5dc6022835ef59573f50d))
* **engine:** reject-escalation up the model ladder ([4b7ce33](https://github.com/darkrun-ai/darkrun/commit/4b7ce33cbf4ee9cc80d4b09adc1a1b4bf7a6b31a))
* **engine:** reject-escalation up the model ladder ([#49](https://github.com/darkrun-ai/darkrun/issues/49)) ([b63792b](https://github.com/darkrun-ai/darkrun/commit/b63792bda5c2c6fa477d9c5787b0e4a56c2ee60c))
* **engine:** save_wip clean-tree gate + unit-scope enforcement at completion ([603c106](https://github.com/darkrun-ai/darkrun/commit/603c1069e2e47241049ac71a184c0a4478a185c2))
* **engine:** session-event stream + OTLP telemetry export ([17873e5](https://github.com/darkrun-ai/darkrun/commit/17873e5c66eab59f5b00c89b4eb409919473e661))
* **engine:** station drop — the keep-or-drop offer at arrival ([e55f7fb](https://github.com/darkrun-ai/darkrun/commit/e55f7fb008c73455506eb85685894e0cee264876))
* **factory:** runtime-verifier run reviewer (the predecessor's strongest gate) ([08d8e45](https://github.com/darkrun-ai/darkrun/commit/08d8e45b0775e60ce7aae045bab3cfe73d4e3602))
* **hosting:** run-level draft PR with ready-at-seal flip + compare-URL fallback ([08d8e45](https://github.com/darkrun-ai/darkrun/commit/08d8e45b0775e60ce7aae045bab3cfe73d4e3602))
* **providers:** behavior contracts spliced into prompts + schema-validated settings ([99f2687](https://github.com/darkrun-ai/darkrun/commit/99f26873b330f51073d6ac25c35c5989fdf87da6))
* **site+desktop:** refreshed desktop screenshots + the harness that makes them reproducible ([7df1a90](https://github.com/darkrun-ai/darkrun/commit/7df1a90642ed6604aafd612242bfc311dde9b5a8))
* **site:** docs search + JSON-LD structured data ([3e0ceaa](https://github.com/darkrun-ai/darkrun/commit/3e0ceaa93691d2bda46a6bbd0b44df8a81f5c1ea))
* **statusline+site:** phase-track pips + the status line on the website + the fable tier ([5a7ae9a](https://github.com/darkrun-ai/darkrun/commit/5a7ae9ac7af4421911b2c84a97f80bcb0da48286))


### Bug Fixes

* **api:** make openapi.json a fixed point of release-please's rewrite ([f3bbf95](https://github.com/darkrun-ai/darkrun/commit/f3bbf957a4ae51d0746dfed79006678250a596e0))
* **api:** make openapi.json a fixed point of release-please's rewrite ([#42](https://github.com/darkrun-ai/darkrun/issues/42)) ([c964465](https://github.com/darkrun-ai/darkrun/commit/c96446584c9e1c5f84953ec92df57a909e4999a1))
* **desktop:** project identity self-heal, --worktree launch, choosable clone path ([c0efbbf](https://github.com/darkrun-ai/darkrun/commit/c0efbbf82d718677187fde831bf26e79a791bcb6))
* **site:** make the feed-date suggestions compile + emit valid formats ([88f4061](https://github.com/darkrun-ai/darkrun/commit/88f40619b36a4df2f03515140385072dba8dc2b6))

## [0.2.1](https://github.com/darkrun-ai/darkrun/compare/v0.2.0...v0.2.1) (2026-06-08)


### Bug Fixes

* propagate the 0.2.0 release bump (unblock all open PRs) ([#20](https://github.com/darkrun-ai/darkrun/issues/20)) ([ae45020](https://github.com/darkrun-ai/darkrun/commit/ae4502037ebb67513017416143c6830a2f77489b))

## [0.2.0](https://github.com/darkrun-ai/darkrun/compare/v0.1.0...v0.2.0) (2026-06-08)


### Features

* darkrun — factory-orchestration engine, design system, website, and plugin ([f6365d8](https://github.com/darkrun-ai/darkrun/commit/f6365d812cf4bd730c9af79147954fd3bf9356cd))
* **git:** complete Phase 1 gix reads — ls_tree, unresolved_paths, list_worktrees ([b33e131](https://github.com/darkrun-ai/darkrun/commit/b33e1318ec367af7571dff1536c4d81cd4b8a434))
* **git:** gix add_paths + checkout_paths — Phase 2 complete ([47ff4b3](https://github.com/darkrun-ai/darkrun/commit/47ff4b364f32ecd0344704c1861f5f17f0c9bcf2))
* **git:** gix create_branch (Phase 2 start) — idempotent ref write ([b5d7267](https://github.com/darkrun-ai/darkrun/commit/b5d7267bfb6fca912b6e6230e2b639c113661cc6))
* **git:** gix engine-protected three-way merge (Phase 4 — the core safety net) ([5346434](https://github.com/darkrun-ai/darkrun/commit/5346434f2807bd7d6ed4c189914e0868a17b380e))
* **git:** gix linked-worktree create/remove (Phase 3 — first gitoxide gap) ([233a16b](https://github.com/darkrun-ai/darkrun/commit/233a16bbae8deb584dc49f30138d7a3fbec6ec64))
* **git:** gix native fetch (Phase 5) — pure-Rust transport, C-free ([6955549](https://github.com/darkrun-ai/darkrun/commit/69555491cb7cec5f78d6db39809cbdbc0dca62c2))
* **git:** gix reads — is_ancestor, refs_have_identical_trees, merge_in_progress ([c074ed0](https://github.com/darkrun-ai/darkrun/commit/c074ed05688954c32044741d1da3f832e717c4d7))
* **git:** hand-rolled write-tree + gix commit (fork-A internals) ([a7e01b6](https://github.com/darkrun-ai/darkrun/commit/a7e01b6dd23af36a49a9ff22f791803a8981c435))
* **git:** scaffold pure-Rust gitoxide backend (Phase 1 foundation) ([885462d](https://github.com/darkrun-ai/darkrun/commit/885462dd8be770255219ee5193bef7720c27a206))
* phase redesign + engine-driven prompts + hooks; Apache-2.0; dark-factory positioning ([5ccf3e9](https://github.com/darkrun-ai/darkrun/commit/5ccf3e9fb6bde91532201c6e33304725c61d8eb2))
* **verify:** objective surface-routed verification + view/visual-review + proof ([60062d9](https://github.com/darkrun-ai/darkrun/commit/60062d96dd94aca99f7cdf8a8d47bfc76b35a5b8))
* **visual:** visual-question + design-direction sessions, screens, and a visual-design step ([db0500e](https://github.com/darkrun-ai/darkrun/commit/db0500e3bc079ca7639471c0939b9a7b2ec3bd3d))


### Bug Fixes

* 0-byte outputs don't satisfy the gate; verify gate/drift loop immunity (predecessor BUGs 1, 3, drift A/B) ([9220710](https://github.com/darkrun-ai/darkrun/commit/92207108953e3bf732d31118726026b03efe607e))
* darkrun show deep-links to the run; stations render in factory order ([655d7a0](https://github.com/darkrun-ai/darkrun/commit/655d7a087c8e7c144e885cd444e4df4312a43195))
* derived_station_phase test needs a unit with a Pass signal (was asserting None-case) ([770ed56](https://github.com/darkrun-ai/darkrun/commit/770ed564a31248490c7c9ac0b5947e14a5792471))
* **plugin:** implement the darkrun hook subcommand so hooks never block tools ([5c3eb12](https://github.com/darkrun-ai/darkrun/commit/5c3eb125cc5105887be856d4b5b831e5e783289e))
* **site:** embed factory corpus in wasm builds + dx 0.7 config ([f8a05f3](https://github.com/darkrun-ai/darkrun/commit/f8a05f3957c4fd09594c3572d0bfff8b04fa7e3e))
* **ui:** stack the run walkthrough — station walker over a centered phase machine ([f4d040b](https://github.com/darkrun-ai/darkrun/commit/f4d040b66b53f3aa54ac84016d75d464c8877a48))

## 0.1.0 — unreleased

The first darkrun: a native Rust engine that drives Runs through a factory's
stations (Frame → Specify → Shape → Build → Prove → Harden for the software
factory), with a desktop review app and a Claude Code plugin.

- **Manager** — a pure-read cursor over `.darkrun/` state, walking the
  six-phase station machine (spec → review → manufacture → audit → reflect →
  checkpoint) across a three-track priority (drift → feedback → run).
- **Full action set** — validation (units-invalid, escalate, safe-repair),
  repair/rollback, external review, and the seal tail.
- **Objective verification** — surface-routed proof (`darkrun verify web`,
  `darkrun bench`) instead of eyeballed review.
- **Reflection** — durable run-level retrospectives.
- **Auto-tune** — run-start right-sizing (quick / bugfix / refactor / full).
- **Drift sweep** — detects mutated locked artifacts and self-heals on revert.
- **Multi-harness** — Claude Code, Cursor, Windsurf, Gemini CLI, OpenCode,
  Kiro, and Codex, each adapted from one capability registry.
