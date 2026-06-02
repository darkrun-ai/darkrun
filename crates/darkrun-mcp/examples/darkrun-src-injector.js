/*
 * darkrun source-map injector — OPT-IN, DEV-ONLY.
 *
 * darkrun resolves an HTML annotation straight to the line that produced it when
 * the marked element carries a `data-darkrun-src="<file>:<line>"` attribute.
 * darkrun does NOT inject that attribute for you — there is no universal way to,
 * because it depends on your framework, bundler, and render path. This file is a
 * minimal, framework-agnostic pattern you copy into YOUR project to emit it.
 *
 * The contract darkrun's resolver enforces (crates/darkrun-mcp/src/annotation.rs,
 * `parse_source_map`):
 *
 *   - the value is exactly  "<repo-relative-file>:<1-based-line>"
 *   - the file part is non-empty; the line part parses as an integer >= 1
 *   - anything else (blank, no line, line 0, garbage) is treated as NOT opted in
 *     and darkrun degrades to selector + outerHTML + the cropped region.
 *
 * So a wrong or empty value costs you nothing — you just fall back to the
 * default, unresolved path. Emit it only when you have a real source location.
 *
 * Two ways to produce the location, in rough order of accuracy:
 *
 *   1) BUILD-TIME (preferred). A JSX transform (Babel/SWC/Vite) already knows
 *      each element's authoring `file:line` — the same data React DevTools uses
 *      for "jump to source". Have it write the attribute next to the element.
 *      Sketch of a Babel visitor:
 *
 *          JSXOpeningElement(path, state) {
 *            const loc = path.node.loc;
 *            if (!loc) return;
 *            const file = relativeToRepoRoot(state.filename); // your helper
 *            const line = loc.start.line;                     // 1-based already
 *            path.node.attributes.push(
 *              t.jsxAttribute(
 *                t.jsxIdentifier("data-darkrun-src"),
 *                t.stringLiteral(`${file}:${line}`),
 *              ),
 *            );
 *          }
 *
 *      Gate the plugin behind NODE_ENV !== "production" so it never ships.
 *
 *   2) RUNTIME (this file). When you can't touch the build, stamp elements as
 *      they mount from whatever source hint your framework exposes. Less precise
 *      than build-time, but zero build wiring. The function below is the shape;
 *      replace `resolveSource` with your own hint lookup.
 */

(function installDarkrunSrcInjector() {
  // Opt-in + dev-only. Bail hard in production so nothing leaks to users.
  const isDev =
    typeof process !== "undefined" &&
    process.env &&
    process.env.NODE_ENV !== "production";
  if (!isDev) return;

  const ATTR = "data-darkrun-src";

  /**
   * Return the repo-relative "file:line" for an element, or null if you have no
   * real source for it. RETURN NULL rather than a guess — darkrun rejects blanks
   * and bad shapes anyway, and a wrong line is worse than none.
   *
   * Replace this body with your framework's source hint. Common sources:
   *   - a fiber / vnode `_debugSource` ({ fileName, lineNumber })
   *   - a `data-source` your template engine already emits
   *   - a WeakMap you populate in your own render wrapper
   */
  function resolveSource(el) {
    // Example: read a hint your render layer parked on the node.
    const hint = el.__source; // { fileName, lineNumber } | undefined
    if (!hint || !hint.fileName || !hint.lineNumber) return null;
    const file = toRepoRelative(hint.fileName);
    const line = hint.lineNumber | 0;
    if (!file || line < 1) return null; // darkrun's exact contract
    return `${file}:${line}`;
  }

  /** Trim an absolute source path down to repo-relative. Adapt to your layout. */
  function toRepoRelative(abs) {
    // Cheap default: strip anything up to and including a "/src/" segment.
    const i = abs.indexOf("/src/");
    return i >= 0 ? abs.slice(i + 1) : abs;
  }

  /** Stamp one element if it has a real source and isn't already tagged. */
  function tag(el) {
    if (!(el instanceof Element) || el.hasAttribute(ATTR)) return;
    const src = resolveSource(el);
    if (src) el.setAttribute(ATTR, src);
  }

  /** Stamp an element and its descendants. */
  function tagTree(root) {
    tag(root);
    if (root.querySelectorAll) root.querySelectorAll("*").forEach(tag);
  }

  // Initial pass once the DOM is ready...
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", () => tagTree(document.body));
  } else {
    tagTree(document.body);
  }

  // ...then keep newly mounted nodes tagged as the app renders.
  const mo = new MutationObserver((records) => {
    for (const r of records) {
      r.addedNodes.forEach((n) => {
        if (n.nodeType === Node.ELEMENT_NODE) tagTree(n);
      });
    }
  });
  mo.observe(document.documentElement, { childList: true, subtree: true });
})();
