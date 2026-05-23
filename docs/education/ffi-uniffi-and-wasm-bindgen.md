# UniFFI and wasm_bindgen

Both tools solve the same core problem: **expose Rust code to a host language that can't directly link to Rust's ABI**. They take different routes because the hosts are different — UniFFI targets Swift/Kotlin/Python over a C ABI, wasm_bindgen targets JavaScript over the WebAssembly ABI. Once you've internalized that one framing, most of the design decisions in each fall out naturally.

This guide goes top-down: the shared problem → UniFFI in depth → wasm_bindgen in depth → side-by-side comparison → how they fit Eafora.

---

## 1. The shared problem

Rust's normal calling convention is unstable and Rust-specific. Even between two Rust crates, the ABI isn't guaranteed across compiler versions. So when you want a Swift app to call a Rust function, you can't just hand Swift a `.rlib` and expect it to work. You need to:

1. **Pick a stable ABI** that both sides can speak.
2. **Translate Rust types** (with their move semantics, borrow rules, monomorphized generics, traits) into something the host language can represent.
3. **Manage memory across the boundary** — who owns a returned `String`? When does a `Vec<u8>` get freed?
4. **Surface errors** in a way that feels natural to the host.
5. **Handle async** if the host has its own concurrency model (Swift's `async/await`, JS's `Promise`).

UniFFI and wasm_bindgen are both **codegen tools** that do this translation for you. You annotate Rust types and functions with macros (or in UniFFI's older style, write a `.udl` file), the tool generates:

- A C-compatible (UniFFI) or WASM-compatible (wasm_bindgen) shim layer in Rust.
- Bindings in the host language (Swift, Kotlin, Python; or TypeScript/JavaScript) that call the shim.

The host code looks idiomatic. The shim is generated, ugly, and not your problem.

---

## 2. UniFFI

### 2.1 What it is

UniFFI is a Mozilla project (originally built for Firefox's application-services). It's a **multi-language FFI generator** that takes a Rust crate and emits bindings for Swift, Kotlin, Python, and Ruby. The wire format is a **custom serialization over a C ABI** — UniFFI defines its own way to serialize a `String` or a `Vec<T>` across the boundary, rather than using Rust's repr or trying to match the host's layout.

### 2.2 The mental model

Imagine your Rust crate as a server, and Swift/Kotlin as clients. UniFFI generates:

- On the Rust side, a set of `extern "C"` functions with simple primitive signatures (pointers, integers, byte buffers).
- On the host side, classes/structs that look natural in Swift and Kotlin, with code that **serializes arguments** into a byte buffer, calls the C function, **deserializes the result**, and translates errors into `throws` / exceptions.

The serialization format is fixed by UniFFI. It's not protobuf or MessagePack — it's a UniFFI-specific layout, but the principle is the same: structured Rust values become `RustBuffer` (a length-prefixed byte buffer with an explicit allocator), get passed by pointer, and the host side reads them back.

### 2.3 The two authoring styles

UniFFI has two ways to declare what crosses the boundary:

**(a) `.udl` files (older)** — a separate IDL file describing the API. The Rust code has to match. This is older and more brittle; you maintain two sources of truth.

**(b) Proc macros (newer, what you'd use now)** — annotate the Rust code directly with `#[uniffi::export]`, `#[derive(uniffi::Record)]`, `#[derive(uniffi::Enum)]`, etc. UniFFI's macros become the source of truth. This is the style you should use for new projects.

```rust
use uniffi;

#[derive(uniffi::Record)]
pub struct CountryStat {
    pub iso3: String,
    pub year: u16,
    pub tfr: f64,
}

#[derive(uniffi::Enum)]
pub enum DataStatus {
    Final,
    Provisional { revision: u8 },
    Estimated,
}

#[derive(thiserror::Error, Debug, uniffi::Error)]
pub enum EaforaError {
    #[error("not found: {iso3}")]
    NotFound { iso3: String },
    #[error("io: {message}")]
    Io { message: String },
}

#[uniffi::export]
pub fn fetch_country(iso3: String) -> Result<CountryStat, EaforaError> {
    // ...
}
```

That's the entire authoring surface for a simple case. No `.udl`, no manual extern blocks.

### 2.4 The build-time pipeline

You add a `build.rs` (or use `uniffi::setup_scaffolding!()`) that generates the C shim. Then you run `uniffi-bindgen generate --library libeafora.dylib --language swift --out-dir generated/` to emit Swift code. Same for Kotlin, Python, etc.

What you end up with on disk:

```
core/
  src/
    lib.rs                    # your annotated Rust
  Cargo.toml
target/
  release/
    libeafora.dylib           # Rust artifact
generated/
  swift/
    eafora.swift              # Swift binding
    eaforaFFI.h               # C header
    eaforaFFI.modulemap       # Swift module map
  kotlin/
    uniffi/eafora/eafora.kt   # Kotlin binding
```

For iOS, you bundle the dylib (or static lib) plus the generated Swift into an **xcframework** — a multi-architecture container Apple's toolchain understands. For Android, you bundle the `.so` files (built per ABI: arm64-v8a, armeabi-v7a, x86_64) into the app's `jniLibs/` plus the generated Kotlin into the project's source set, typically packaged as an AAR.

### 2.5 The type system mapping

UniFFI defines a fixed set of types that can cross the boundary. This is the crucial constraint to internalize — **it is not "any Rust type"**.

| Rust                         | Swift                          | Kotlin                          |
| ---------------------------- | ------------------------------ | ------------------------------- |
| `bool`                       | `Bool`                         | `Boolean`                       |
| `i8`/`i16`/`i32`/`i64`       | `Int8`/`Int16`/`Int32`/`Int64` | `Byte`/`Short`/`Int`/`Long`     |
| `u8`/`u16`/`u32`/`u64`       | `UInt8`/`UInt16`/...           | `UByte`/`UShort`/`UInt`/`ULong` |
| `f32`/`f64`                  | `Float`/`Double`               | `Float`/`Double`                |
| `String`                     | `String`                       | `String`                        |
| `Vec<u8>`                    | `Data`                         | `ByteArray`                     |
| `Vec<T>`                     | `[T]`                          | `List<T>`                       |
| `HashMap<String, V>`         | `[String: V]`                  | `Map<String, V>`                |
| `Option<T>`                  | `T?`                           | `T?`                            |
| `#[derive(uniffi::Record)]`  | `struct`                       | `data class`                    |
| `#[derive(uniffi::Enum)]`    | `enum` (with assoc values)     | sealed class hierarchy          |
| `#[uniffi::export] impl`     | class with methods             | class with methods              |
| `Arc<dyn Trait>`             | protocol-conforming class      | interface-implementing class    |
| `Result<T, E>`               | `throws` function              | function throwing exception     |

What you **cannot** cross-boundary directly:
- Generic type parameters (UniFFI is monomorphized at the boundary; you have to pick a concrete `T`).
- Lifetimes (everything crossing the boundary is owned).
- Borrowed references (`&str`, `&[T]`) — they get converted to owned `String`/`Vec`.
- Closures returning data (callbacks are supported via traits, but the shape is constrained).

### 2.6 Records vs Objects — the most important distinction

UniFFI has two ways to expose a Rust struct, and conflating them is the most common mistake:

- **`uniffi::Record`** = **value type, copied by value across the boundary**. Every field is serialized into the byte buffer. The host gets its own copy. Mutations on the host side don't propagate to Rust. Use this for simple data structs (DTOs).

- **`uniffi::Object`** = **reference type, lives in Rust, host holds an opaque handle**. The host sees a class with methods. Each method call goes back across the FFI to Rust. Use this for objects with internal state, expensive construction, or methods that mutate.

```rust
#[derive(uniffi::Record)]
pub struct CountryStatRecord { ... }   // value: copied across

#[derive(uniffi::Object)]
pub struct StatisticEngine { ... }     // reference: handle on host

#[uniffi::export]
impl StatisticEngine {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> { ... }

    pub fn lookup(&self, iso3: String, year: u16) -> Option<CountryStatRecord> { ... }
}
```

For Eafora: `CountryStat`, `DataStatus`, `Indicator` etc. are records. `StatisticEngine`, `RenderContext`, `IngestionPipeline` are objects.

### 2.7 Errors

`#[derive(uniffi::Error)]` on a `thiserror`-style enum maps to:
- Swift: `throws`. Each variant becomes a case of an `Error`-conforming enum.
- Kotlin: `Exception` subclass per variant.

The host-side code reads naturally:

```swift
do {
    let stat = try engine.lookup(iso3: "USA", year: 2024)
} catch EaforaError.NotFound(let iso3) {
    print("missing \(iso3)")
}
```

You write idiomatic `Result<T, E>` and `?` in Rust. UniFFI translates. (This is the default we landed on for Eafora.)

### 2.8 Async

UniFFI has supported async since ~late 2023. Annotate `async fn` functions and they map to:
- Swift: `async throws` — `try await engine.fetch(...)`.
- Kotlin: `suspend fun` — works with coroutines.

Underneath, UniFFI runs a Rust runtime (you choose tokio) and bridges the Future to the host's executor. The host's `await` suspends; the Rust future completes; UniFFI marshals the result back and resumes the host coroutine/Task.

This is great for Eafora because ingestion calls (HTTP, file I/O) are naturally async on the Rust side and you get to keep them that way across the boundary.

### 2.9 Callbacks (Rust calling host)

`#[uniffi::export(callback_interface)]` on a trait lets Rust invoke host code. The host implements the trait in Swift/Kotlin, passes an instance, Rust calls its methods. This is how you'd wire up "Rust core asks the host to log this" or "Rust core asks the host to fetch a URL" if you wanted to delegate I/O to the host's networking.

### 2.10 Memory management

For records: serialize-deserialize means each side owns its copy. No shared ownership.

For objects: UniFFI uses `Arc<T>` on the Rust side; the host holds a handle that increments the Arc on creation and decrements on `deinit` (Swift) or `close()`/finalizer (Kotlin). This is one of the few places where you have to think — Kotlin classes implementing `AutoCloseable` is the recommended pattern for explicit lifecycle, otherwise you're at the mercy of GC.

### 2.11 Common gotchas

- **No `&self` lifetime crossing**: methods on objects take `&self` internally, but the FFI-visible signature loses the lifetime; the host can't hold a reference to a field of an object.
- **Vec serialization cost**: a `Vec<u8>` of 50 MB gets fully serialized across the FFI on every call. For large geometry data you don't pass the bytes; you keep them inside an object and expose query methods.
- **String encoding**: UniFFI requires valid UTF-8. A panic on invalid UTF-8 in Rust will unwind into the FFI boundary and crash. Validate inputs at the boundary.
- **`uniffi-bindgen` versioning**: the bindgen tool and the runtime crate must match versions exactly. Pin both.

---

## 3. wasm_bindgen

### 3.1 What it is

wasm_bindgen is from the rustwasm working group. It generates **JavaScript bindings** for Rust code compiled to WebAssembly. Unlike UniFFI, it's single-target (JS only) but goes deep — it knows how to round-trip JavaScript objects, DOM types, `Promise`s, closures, etc.

The output ABI is **WebAssembly's interface**, which is itself just integers and floats. Strings, arrays, objects all have to be marshaled across this very narrow channel. wasm_bindgen handles it.

### 3.2 The mental model

WebAssembly itself can only pass `i32`, `i64`, `f32`, `f64` across the function boundary (the more recent `WebAssembly.Reference` type extension lets you pass opaque `externref`s, but treat that as an optimization). Anything else — strings, arrays, objects — has to be:

1. **Allocated in WASM linear memory** by the Rust side, returned as a pointer + length pair.
2. **Read out by JavaScript** via a `DataView` over the WASM module's memory.
3. **Converted** to a JS string/array/object.

wasm_bindgen generates **glue JS** that does this round-tripping. You don't see it; you just call `engine.lookup("USA", 2024)` from JS and it works.

For object-typed JS values flowing **into** Rust (e.g. a DOM node), wasm_bindgen maintains a side table on the JS side mapping integer handles to actual JS objects. Rust holds the handle (a `JsValue`); when Rust wants to call a method on it, it crosses back to JS via a generated import.

### 3.3 Authoring

```rust
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct StatisticEngine {
    inner: eafora_core::StatisticEngine,
}

#[wasm_bindgen]
impl StatisticEngine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> StatisticEngine {
        StatisticEngine { inner: eafora_core::StatisticEngine::new() }
    }

    pub fn lookup(&self, iso3: &str, year: u16) -> Option<CountryStat> {
        self.inner.lookup(iso3, year)
    }
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct CountryStat {
    pub year: u16,
    pub tfr: f64,
    iso3: String,
}

#[wasm_bindgen]
impl CountryStat {
    #[wasm_bindgen(getter)]
    pub fn iso3(&self) -> String { self.iso3.clone() }
}
```

Note the asymmetries with UniFFI:
- Numeric fields can be `pub` and they auto-generate getters/setters.
- `String` fields can't be `pub` because they need to be cloned across the boundary; you write a getter.
- `&str` arguments work directly (wasm_bindgen will copy the string into linear memory and pass the pointer).
- `Option<T>` for value types works for primitives; for structs it goes through `JsValue`.

### 3.4 Build-time pipeline

`wasm-pack build` is the conventional wrapper. It runs `cargo build --target wasm32-unknown-unknown`, then runs `wasm-bindgen` over the resulting `.wasm` to emit:

```
pkg/
  eafora_bg.wasm       # the actual wasm module (post-bindgen)
  eafora.js            # JS glue
  eafora.d.ts          # TypeScript declarations
  eafora_bg.wasm.d.ts  # declarations for the raw exports
  package.json         # ready to publish or import
```

You import it from JS as a normal module:

```typescript
import init, { StatisticEngine } from "./pkg/eafora.js";

await init();  // loads the .wasm, instantiates it, wires up imports/exports
const engine = new StatisticEngine();
const stat = engine.lookup("USA", 2024);
```

For Leptos specifically: Leptos uses wasm_bindgen under the hood; you don't usually call `wasm-pack` directly. `cargo leptos build` orchestrates the wasm-bindgen step plus CSS/asset bundling.

### 3.5 The type system mapping

| Rust                       | JavaScript                                           |
| -------------------------- | ---------------------------------------------------- |
| `bool`                     | `boolean`                                            |
| `i8`...`i32`/`u8`...`u32`  | `number`                                             |
| `i64`/`u64`                | `BigInt` (or `number` with `i64-as-number` opt-in)   |
| `f32`/`f64`                | `number`                                             |
| `String`/`&str`            | `string`                                             |
| `Vec<u8>`                  | `Uint8Array` (with `js_sys::Uint8Array` for views)   |
| `Vec<T>` (T: primitive)    | typed array                                          |
| `Vec<T>` (T: struct)       | `Array` of objects                                   |
| `Option<T>`                | `T \| undefined`                                     |
| `Result<T, JsValue>`       | throws on `Err`                                      |
| `#[wasm_bindgen]` struct   | JS class                                             |
| `JsValue`                  | any JS value (opaque handle)                         |
| `js_sys::Promise`          | `Promise`                                            |
| Async fn returning `T`     | `Promise<T>`                                         |
| Closures (`&dyn Fn(...)`)  | JS function (with `Closure` lifetime management)     |

### 3.6 The "ownership lives on the Rust side" pattern

Same principle as UniFFI objects: large data (geometry buffers, SQLite databases) lives in Rust-side WASM linear memory. JS holds an opaque handle (the wasm-bindgen-generated class instance). Method calls jump into WASM; strings/numbers come back.

For Eafora the pattern looks like:

```rust
#[wasm_bindgen]
pub struct ArtifactCache {
    sqlite: rusqlite::Connection,
    geometry: HashMap<String, Geometry>,
}

#[wasm_bindgen]
impl ArtifactCache {
    pub fn lookup_country(&self, iso3: &str, year: u16) -> Option<CountryStat> { ... }
    pub fn render_polygons(&self, gpu_buffer: &js_sys::Uint8Array) { ... }
}
```

The 50 MB SQLite file sits in linear memory. JS never sees the bytes. JS calls `cache.lookup_country("USA", 2024)` and gets a small `CountryStat` back.

### 3.7 Async

`async fn` in Rust → `Promise<T>` in JS. Backed by `wasm-bindgen-futures` which adapts Rust futures to the JS event loop.

```rust
#[wasm_bindgen]
pub async fn fetch_artifact(url: String) -> Result<JsValue, JsValue> {
    let response = gloo_net::http::Request::get(&url).send().await?;
    let bytes: Vec<u8> = response.binary().await?;
    Ok(JsValue::from(bytes))
}
```

```typescript
const data = await fetch_artifact("https://cdn.eafora.org/...");
```

This is critical for Eafora's web client because all artifact loading is async fetch over HTTP.

### 3.8 Errors

Convention: return `Result<T, JsValue>`. The `Err` variant becomes a JS exception. You typically construct `JsValue::from_str("error message")` or `JsError::new("...")`.

Unlike UniFFI, there's no automatic per-variant exception type. If you want typed errors on the JS side, you serialize a structured object (often via `serde-wasm-bindgen`) and throw that.

### 3.9 Closures and callbacks

Passing a JS function to Rust uses `&js_sys::Function` (call once) or `Closure<dyn FnMut(...)>` (Rust-side handle to a JS function with managed lifetime). Passing a Rust closure to JS requires `Closure::wrap(...)` and explicit lifetime management — closures are one of the most error-prone areas because if Rust drops a closure that JS still holds a reference to, you get a runtime crash on call.

### 3.10 Memory management

JS holds a handle. The wasm-bindgen-generated class has a `free()` method. By default the JS class is **not** garbage-collected with respect to its WASM memory — you must call `engine.free()` to release the linear-memory allocation, otherwise it leaks for the lifetime of the WASM instance.

There's a newer "weakref" support that hooks into JS's `FinalizationRegistry` to call `free()` automatically, but it requires opt-in and the GC timing is unpredictable. For long-lived objects (the singleton statistic engine for the page) this doesn't matter; for short-lived per-render objects you want explicit `free()` or the weakref path.

### 3.11 serde-wasm-bindgen

For arbitrary serializable Rust types, `serde-wasm-bindgen` lets you skip writing `#[wasm_bindgen]` for every record and just have JS see them as plain JS objects (round-tripped through serde JSON-like serialization). It's looser typed (TypeScript declarations are weaker) but much less ceremony for DTOs.

```rust
#[derive(Serialize, Deserialize)]
pub struct CountryStat { ... }

#[wasm_bindgen]
pub fn lookup(iso3: &str, year: u16) -> Result<JsValue, JsValue> {
    let stat: CountryStat = ...;
    Ok(serde_wasm_bindgen::to_value(&stat)?)
}
```

For Eafora you'd want to be deliberate — use `#[wasm_bindgen]` for the public surface (gets you typed classes and methods) and reserve serde-wasm-bindgen for bulk DTOs where typed classes would be ceremony.

---

## 4. Side-by-side

| Concern                  | UniFFI                                            | wasm_bindgen                                        |
| ------------------------ | ------------------------------------------------- | --------------------------------------------------- |
| Targets                  | Swift, Kotlin, Python, Ruby                       | JavaScript / TypeScript                             |
| Wire ABI                 | C ABI + custom byte-buffer serialization          | WebAssembly + linear-memory pointer/length          |
| Codegen direction        | Rust + multi-language out                         | Rust + JS glue out                                  |
| Type model               | Records (value) vs Objects (handle)               | All `#[wasm_bindgen]` types are handle-based; primitives are by-value |
| Generic types            | Monomorphize at boundary                          | Monomorphize at boundary                            |
| Async                    | yes — maps to `async`/`suspend`                   | yes — maps to `Promise`                             |
| Errors                   | `Result<T, E>` → typed `throws`/exception         | `Result<T, JsValue>` → JS throw (untyped by default)|
| Callbacks                | Trait with `callback_interface`                   | `Closure`, `js_sys::Function`                       |
| Memory model             | Arc on Rust side; host handle inc/dec             | JS handle; explicit `free()` (or weakref opt-in)    |
| Build tool               | `uniffi-bindgen`                                  | `wasm-pack` / `wasm-bindgen` CLI / `cargo leptos`   |
| Bundle artifact          | xcframework (iOS), AAR (Android), wheel (Python)  | `.wasm` + `.js` + `.d.ts` package                   |
| Where the code runs      | Native, dynamically/statically linked             | Browser WASM VM (or Node, Deno, etc.)               |
| Threading                | Same as native — full tokio runtime               | Single-threaded by default; `wasm-bindgen-rayon` for SAB-based threads |
| Strictness of types      | Strong — bindgen output is typed Swift/Kotlin     | Strong if you use `#[wasm_bindgen]` types; loose with serde-wasm-bindgen |
| Source-of-truth style    | Proc macros on Rust (modern) or `.udl` (legacy)   | Proc macros on Rust                                 |

The conceptual difference that matters most: **UniFFI assumes a fixed type universe**, **wasm_bindgen lets you reach into the host language directly via `JsValue`**. wasm_bindgen is "open-world" — you can grab arbitrary JS — because JS has no type system and the boundary is loose. UniFFI is "closed-world" because Swift and Kotlin both have stricter type systems and the binding has to commit to types up front.

---

## 5. How they fit Eafora

Recall the architecture: a `core/` Rust crate holds all data, render, ingest, and statistic logic. Three clients consume it:

```
core/ (Rust crate)
  ├── via UniFFI ──► iOS app (SwiftUI calls Swift bindings)
  ├── via UniFFI ──► Android app (Compose calls Kotlin bindings)
  └── via wasm_bindgen ──► Web app (Leptos in WASM calls Rust directly...
                             see below)
```

The web case is special. Leptos runs **inside the same WASM module** as the core. There's no FFI boundary in the traditional sense — the Leptos UI is itself Rust compiled to WASM, calling `core` as a normal Rust crate. wasm_bindgen only enters the picture **at the boundary between WASM and the browser** (DOM access, fetch, console.log, GPU APIs). You don't wasm_bindgen-export the core's internal API; you wasm_bindgen-export only what crosses to JS.

So the actual picture:

```
core/  ──Rust internal API──►  web/ (Leptos)
                                 │
                                 └─ wasm_bindgen ──► browser (DOM, fetch, WebGPU, IndexedDB)

core/ ──UniFFI──► xcframework ──► ios/ (SwiftUI)
core/ ──UniFFI──► AAR ──────────► android/ (Compose)
```

This asymmetry is a feature, not a wart: the web client gets to call into core with no marshalling cost (it's all Rust→Rust, monomorphized at compile time). The mobile clients pay UniFFI's marshalling cost on every call, which is why object-typed handles + coarse-grained method calls matter for them.

**Practical implication for the core crate's API design:** design the public surface with UniFFI's constraints in mind (no generics, no lifetimes, primitive + record + Arc-handle types), because that surface has to be UniFFI-exportable. The web client gets the same surface for free; it doesn't constrain the web case.

### What you'll write in the core crate

```rust
// core/src/ffi/mod.rs — the FFI boundary module
// This is the only place UniFFI/wasm_bindgen attributes appear.

#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
pub struct CountryStat { ... }

#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
#[cfg_attr(feature = "wasm", wasm_bindgen)]
pub struct StatisticEngine { ... }

#[cfg_attr(feature = "uniffi", uniffi::export)]
#[cfg_attr(feature = "wasm", wasm_bindgen)]
impl StatisticEngine {
    pub fn lookup(&self, iso3: String, year: u16) -> Option<CountryStat> { ... }
}
```

Cargo features select which generator is active per build. The `uniffi` feature is on for the iOS/Android dylib build; the `wasm` feature is on for the WASM build. The internal `core` API (used by other Rust code in the workspace) stays clean of both attribute sets.

---

## 6. What to study next, in depth

If you want to keep learning by reading source rather than docs:

- **UniFFI**: read the `uniffi-rs` repo's `examples/` directory. The `arithmetic`, `rondpoint`, and `geometry` examples cover ~80% of real usage patterns. Then read `uniffi-bindgen-swift`'s code generator to see exactly what Swift comes out — once you've seen the generated Swift for a `Record` and an `Object`, the mental model is locked in.

- **wasm_bindgen**: read the `wasm-bindgen` book's "Reference > The `#[wasm_bindgen]` attribute" section, then look at the `examples/` directory in the rustwasm/wasm-bindgen repo. The `dom` and `webgl` examples show the JS-object-handle pattern; the `closures` example shows the trickiest part of the API.

- **For Eafora specifically**: when we get to the per-platform plans, the right exercise is to write the same trivial `Engine::lookup(iso3, year) -> CountryStat` function and walk it through both bindings end-to-end — so you've felt the build pipelines, seen the generated code, and exercised the runtime. That's the fastest way to lock in the abstractions.
