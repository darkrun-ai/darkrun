// darkrun web collector — runs in the page (via CDP Runtime.evaluate) and
// returns a JSON object of { dom: DomSnapshot, vitals: PageVitals } that the
// Rust audit analyzers consume. Kept dependency-free so it runs in any Chrome.
(() => {
  // --- contrast --------------------------------------------------------------
  function parseColor(str) {
    const m = str && str.match(/rg\w*\(([^)]+)\)/);
    if (!m) return null;
    const parts = m[1].split(",").map((s) => parseFloat(s.trim()));
    return { r: parts[0], g: parts[1], b: parts[2], a: parts.length > 3 ? parts[3] : 1 };
  }
  function lum(c) {
    const f = (v) => {
      v /= 255;
      return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b);
  }
  function ratio(fg, bg) {
    const a = lum(fg) + 0.05;
    const b = lum(bg) + 0.05;
    return a > b ? a / b : b / a;
  }
  function effectiveBg(el) {
    let node = el;
    while (node && node.nodeType === 1) {
      const c = parseColor(getComputedStyle(node).backgroundColor);
      if (c && c.a > 0) return c;
      node = node.parentElement;
    }
    return { r: 255, g: 255, b: 255, a: 1 };
  }
  function hasText(el) {
    for (const n of el.childNodes) {
      if (n.nodeType === 3 && n.textContent.trim().length) return true;
    }
    return false;
  }
  const text_contrasts = [];
  const seenC = new Set();
  document.querySelectorAll("body *").forEach((el) => {
    if (text_contrasts.length >= 200) return;
    if (!hasText(el)) return;
    const st = getComputedStyle(el);
    if (st.visibility === "hidden" || st.display === "none") return;
    const fg = parseColor(st.color);
    if (!fg) return;
    const bg = effectiveBg(el);
    const r = ratio(fg, bg);
    const label = el.tagName.toLowerCase();
    if (seenC.has(label + r.toFixed(2))) return;
    seenC.add(label + r.toFixed(2));
    text_contrasts.push({ label, ratio: Math.round(r * 100) / 100 });
  });

  // --- touch targets ---------------------------------------------------------
  const touch_targets = [];
  document.querySelectorAll("a,button,input,select,textarea,[role=button]").forEach((el) => {
    const st = getComputedStyle(el);
    if (st.display === "none" || st.visibility === "hidden") return;
    if (el.type === "hidden") return;
    const rect = el.getBoundingClientRect();
    touch_targets.push({
      label: el.tagName.toLowerCase(),
      width: Math.round(rect.width),
      height: Math.round(rect.height),
    });
  });

  // --- images ----------------------------------------------------------------
  const images = [];
  document.querySelectorAll("img").forEach((img) => {
    const alt = img.getAttribute("alt");
    const presentation = img.getAttribute("role") === "presentation" || img.getAttribute("aria-hidden") === "true";
    images.push({
      label: (img.getAttribute("src") || "").slice(0, 80),
      has_alt: presentation || (alt != null && alt.trim().length > 0),
    });
  });

  // --- reduced motion (stylesheet scan) -------------------------------------
  let honors_reduced_motion = false;
  for (const sheet of document.styleSheets) {
    try {
      for (const rule of sheet.cssRules) {
        if (rule.media && /prefers-reduced-motion/.test(rule.media.mediaText)) {
          honors_reduced_motion = true;
        }
      }
    } catch (e) {
      /* cross-origin sheet — skip */
    }
  }

  // --- landmarks -------------------------------------------------------------
  const landmarks = document.querySelectorAll(
    "main,nav,header,footer,aside,[role=main],[role=navigation],[role=banner],[role=contentinfo],[role=complementary]"
  );
  const has_main_landmark = !!document.querySelector("main,[role=main]");

  // --- keyboard reachability -------------------------------------------------
  const interactive = document.querySelectorAll("a[href],button,input,select,textarea,[tabindex]");
  let interactive_total = 0;
  let keyboard_focusable = 0;
  interactive.forEach((el) => {
    if (el.type === "hidden") return;
    interactive_total++;
    const ti = el.getAttribute("tabindex");
    const disabled = el.disabled === true;
    if (!disabled && ti !== "-1") keyboard_focusable++;
  });

  // --- document metadata -----------------------------------------------------
  const has_document_title = !!(document.title && document.title.trim().length);
  const has_lang = !!(document.documentElement.getAttribute("lang") || "").trim();

  // --- vitals ----------------------------------------------------------------
  const nav = performance.getEntriesByType("navigation")[0] || {};
  const fcpEntry = performance.getEntriesByName("first-contentful-paint")[0];
  let lcp = null;
  const lcpEntries = performance.getEntriesByType("largest-contentful-paint");
  if (lcpEntries.length) lcp = lcpEntries[lcpEntries.length - 1].startTime;
  let cls = 0;
  for (const e of performance.getEntriesByType("layout-shift")) {
    if (!e.hadRecentInput) cls += e.value;
  }
  const mem = performance.memory ? performance.memory.usedJSHeapSize : null;

  const vitals = {
    ttfb: nav.responseStart != null && nav.requestStart != null ? Math.round(nav.responseStart - nav.requestStart) : null,
    fcp: fcpEntry ? Math.round(fcpEntry.startTime) : null,
    lcp: lcp != null ? Math.round(lcp) : null,
    cls: Math.round(cls * 1000) / 1000,
    inp: null,
    transfer_size: nav.transferSize != null ? nav.transferSize : null,
    js_heap_used: mem,
  };

  return JSON.stringify({
    dom: {
      text_contrasts,
      touch_targets,
      images,
      honors_reduced_motion,
      landmark_count: landmarks.length,
      has_main_landmark,
      keyboard_focusable,
      interactive_total,
      has_document_title,
      has_lang,
    },
    vitals,
  });
})()
