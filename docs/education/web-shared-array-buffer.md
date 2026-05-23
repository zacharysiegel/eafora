# Cross-origin isolation, COOP, COEP, and SharedArrayBuffer

These four concepts are inseparable on the modern web. `SharedArrayBuffer` is the feature that motivated the gating. `Cross-Origin-Opener-Policy` (COOP) and `Cross-Origin-Embedder-Policy` (COEP) are the two HTTP response headers that unlock it. "Cross-origin isolation" is the page-level state those headers put your document into. Once you understand why the gate exists, the design choices around the headers — and the secondary costs they impose on your application — fall out cleanly.

This guide goes top-down: what `SharedArrayBuffer` is and why it matters → the Spectre-era reason it's gated → the two headers and what each closes off → the application-level costs of turning the gate on → how Eafora resolves the tradeoff.

---

## 1. What SharedArrayBuffer is

In JavaScript, `SharedArrayBuffer` is a chunk of memory that can be read and written by both the **main thread and Web Workers** at the same time. Without it, threads in the browser can only talk via `postMessage`, which serializes data on every hop. With it, multiple threads literally observe the same bytes, with `Atomics.*` operations available for synchronization.

For WebAssembly specifically, `SharedArrayBuffer` is what lets a multi-threaded program — anything using `pthread`, Rust's `std::thread`, or crates like `rayon` — actually run on multiple cores in the browser. Without it, your WASM is single-threaded by construction. There is no way to run code on more than one core.

A toy mental model:

| Without `SharedArrayBuffer` | With `SharedArrayBuffer` |
|---|---|
| Worker A and Worker B post copies of a 10 MB buffer back and forth, ~1 ms per hop | Worker A and Worker B index into the same 10 MB region in memory; reads and writes are nanoseconds, locking is via `Atomics.wait` / `Atomics.notify` |

`SharedArrayBuffer` is the building block; `Atomics`, `wasm-bindgen-rayon`, `wasm-thread`, and any threaded portable runtime in the browser sit on top of it.

---

## 2. Why it's gated

In January 2018, Spectre and Meltdown landed and revealed a class of timing-based side-channel attacks that bypassed every previous web security model. Spectre in particular was exploitable through high-resolution timers: by measuring how long memory accesses took with extreme precision, malicious JavaScript on a page could leak bytes from cross-origin contexts that lived in the same browsing process.

`SharedArrayBuffer` is a precision-timer source. When two threads share memory, one thread can write a counter in a tight loop while the other reads it; the rate of change is a clock with sub-nanosecond resolution. That clock plus speculative-execution gadgets is enough to mount a Spectre attack. So all major browsers **disabled `SharedArrayBuffer` immediately** in early 2018.

It came back, but only for pages that prove they're **cross-origin isolated** — meaning there's no cross-origin context in the same browsing-context group that a Spectre-style attack could exfiltrate from. Cross-origin isolation is enforced by the browser when both of these HTTP response headers are set on the document:

- `Cross-Origin-Opener-Policy: same-origin`
- `Cross-Origin-Embedder-Policy: require-corp`

If both are present and the document is rendered, the JavaScript flag `crossOriginIsolated === true`, and `SharedArrayBuffer` is available again.

The general security principle: a page that wants the dangerous primitive (`SharedArrayBuffer`) must accept stricter rules about who it can talk to and what it can load, ensuring the timing-attack surface is contained.

---

## 3. The two headers

These headers don't do the same job. Each closes a different escape hatch.

### 3.1 `Cross-Origin-Opener-Policy: same-origin` (COOP)

COOP governs the **opener relationship between browsing contexts**. Normally, when window A opens window B (via `window.open()` or a regular link with `target="_blank"`), the two windows can talk to each other through `window.opener` and `postMessage`, even if their origins differ. That cross-origin communication channel is the escape hatch.

With `Cross-Origin-Opener-Policy: same-origin`, the browser severs that link whenever origins differ:

- A page on `eafora.org` opens `https://example.com/foo` → `window.opener` is `null` in the new window; the original page can't reach back.
- A page on `eafora.org` is opened by `https://example.com/bar` → same: severed.
- Same-origin opens stay connected normally.

The effect for the page setting this header: it can only meaningfully `postMessage` with same-origin pages, and other-origin pages can never use it as an opener-shaped backchannel.

### 3.2 `Cross-Origin-Embedder-Policy: require-corp` (COEP)

COEP governs the **subresources your page is allowed to load**. Every cross-origin resource (script, image, font, iframe, fetch response) must explicitly opt in, either by serving its own `Cross-Origin-Resource-Policy: cross-origin` header or by passing CORS preflight. Anything that doesn't opt in is **blocked** outright.

Concretely, with `Cross-Origin-Embedder-Policy: require-corp`:

- An `<img src="https://i.imgur.com/foo.jpg">` will fail to load unless imgur sends `Cross-Origin-Resource-Policy: cross-origin` (which most third-party image hosts do not, by default).
- A `<script src="https://cdn.example/lib.js">` will fail to load unless the CDN sends a CORP header or you fetch it with `crossorigin="anonymous"` and the CDN passes CORS.
- An iframe must opt in (via CORP) or be on the same origin.
- A `fetch()` response must include CORP or pass CORS, or the response is rejected.

The effect: nothing cross-origin loads "by accident." Anything you embed must have explicitly said "yes, you may embed me." This closes the "load a sensitive cross-origin resource and read its memory through Spectre" path.

### 3.3 Combined effect

Setting both headers puts the page in **cross-origin isolated** mode:

- It can only freely talk to same-origin pages (COOP).
- It can only load cross-origin subresources that explicitly opted in (COEP).
- The browser sets `crossOriginIsolated = true` and unlocks `SharedArrayBuffer`, `performance.now()` at full precision, and `Atomics.wait`.

The cost is the constraint surface above. The benefit is real shared-memory parallelism.

---

## 4. What turning this on costs

The day-to-day developer experience inside cross-origin isolation has three categories of friction:

### 4.1 Subresource breakage

The most visible cost. Modern web apps load many cross-origin resources: fonts from Google Fonts, scripts from npm CDNs, analytics from third-party hosts, embeds (YouTube, Twitter, Stripe), images from CDNs. Most of them do **not** ship `Cross-Origin-Resource-Policy: cross-origin` by default. Enabling COEP without auditing the page typically breaks half of it.

Workarounds:

- Self-host or proxy the resources you can't get CORP headers on.
- Use the `Cross-Origin-Embedder-Policy: credentialless` mode (newer, more permissive — credentials are stripped from cross-origin requests, which is enough for some browsers to consider isolation safe).
- Apply `crossorigin="anonymous"` on `<script>`/`<img>`/`<link>` tags and rely on CORS, where the third party allows it.

None of these are zero-effort. A working cross-origin-isolated page is one where every third-party dependency has been deliberately validated.

### 4.2 Embeddability is reduced

If your page sets COOP/COEP, embedding it in **someone else's page** is harder. The embedder needs the same headers, plus the embedded page must serve `Cross-Origin-Resource-Policy: cross-origin` so the embedder is allowed to include it. The default workflow ("just drop an iframe in your blog post") doesn't work without coordination.

For a content-shaped product where being widely embedded matters — a journalism widget, a research-portal extension, a school-LMS module — this is a real distribution cost.

### 4.3 Tooling and analytics gotchas

A long tail of issues:

- Plausible Analytics, Sentry, etc. sometimes serve their scripts without CORP. Their scripts mostly work, but error-reporting endpoints can break.
- Service workers and document.cookie behave slightly differently across origin-isolation boundaries.
- Some payment processors (Stripe Elements) require iframes that have to explicitly support COEP integration; older integration paths break.

None of these are unfixable, but each one is a small debugging tax.

---

## 5. The trade for Eafora

For Eafora the question is: do we want `SharedArrayBuffer` (and therefore multi-threaded WASM via `wasm-bindgen-rayon` or `wasm-thread`) badly enough to take on the costs above?

Look at what we actually do in WASM:

| Workload | CPU-bound? | Worth threading? |
|---|---|---|
| Parse the SQLite + FlatGeobuf artifact at startup | Briefly, ~10–100 ms once | Not really; one-shot, latency-tolerable, can hide behind a fade-in |
| Compute hover hit-tests against the country R-tree | No, microseconds per frame | No |
| Drive per-frame camera animation | No, microseconds per frame | No |
| Color-map ~200 country fills | No, microseconds per frame | No |
| Time-series chart computations on country click | Maybe, milliseconds | If it ever matters: spawn a Worker via `postMessage` with a copied buffer |

Multi-threaded WASM via `SharedArrayBuffer` would let some of those run on multiple cores. None of them are CPU-bound enough that we'd notice the speedup. By contrast, **embeddability** is a real product question:

- A journalist dropping `<iframe src="https://eafora.org/country/jp">` into an article.
- A pro-natal nonprofit putting Eafora as a widget in their explainer page.
- A UN demography portal linking out to Eafora as an embedded view.
- A school's LMS embedding a fertility-trends widget.

Each of those is a plausible v2+ distribution channel and each requires the page to be **embeddable**. Cross-origin isolation makes all of them harder.

So Eafora's architectural call is:

> **Skip `SharedArrayBuffer`. Stay single-threaded WASM. Keep the page embeddable.**

Encoded in `docs/architecture/overview.md` as: *"Threading: single-threaded WASM. We **do not** use `SharedArrayBuffer` and therefore do not require `Cross-Origin-Opener-Policy: same-origin` + `Cross-Origin-Embedder-Policy: require-corp` headers."*

If a future feature genuinely needs CPU parallelism — say a heavy demographic projection that has to run in the browser — the right move is then to host the compute-intensive surface on a separate origin (`compute.eafora.org` or similar) with the headers turned on, and leave the main `eafora.org` open for embedding. That's a v3+ concern; this architecture doesn't preclude it.

---

## Further reading

- [MDN: SharedArrayBuffer](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/SharedArrayBuffer)
- [MDN: Cross-Origin-Opener-Policy](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Cross-Origin-Opener-Policy)
- [MDN: Cross-Origin-Embedder-Policy](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Cross-Origin-Embedder-Policy)
- [web.dev: making your website "cross-origin isolated" with COOP and COEP](https://web.dev/articles/coop-coep)
- [Chromium: SharedArrayBuffer updates and the requirement of cross-origin isolation](https://developer.chrome.com/blog/enabling-shared-array-buffer)
