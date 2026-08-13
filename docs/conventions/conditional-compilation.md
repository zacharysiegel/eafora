# Conditional compilation conventions

Gate a block of target- or feature-specific items in a single `#[cfg(...)]`-ed submodule, not with `#[cfg]` repeated on every item. The `#[cfg]` sits on the `mod` and on the `pub use` re-exports; the module body carries no cfg attributes. When both sides of the condition need an implementation, write a parallel module for the complement and re-export the shared public items from whichever is active. Shared types stay at file level; only the gated items move into the modules.

## Example

```rust
// One gate per module + one per re-export, instead of a #[cfg] on every item below.
#[cfg(feature = "hydrate")]
pub use hydrate::regions_expand_on_hover;
#[cfg(not(feature = "hydrate"))]
pub use ssr::regions_expand_on_hover;

#[cfg(feature = "hydrate")]
mod hydrate {
    use base64::Engine;                              // target-specific imports scoped here, not at file top
    // ... store, keys, accessors: no cfg attributes ...
    pub fn regions_expand_on_hover() -> bool { /* real localStorage read */ }
}

#[cfg(not(feature = "hydrate"))]
mod ssr {
    pub fn regions_expand_on_hover() -> bool { true }   // server fallback
}
```

Callers stay cfg-agnostic: they call the one re-exported name regardless of build.

## Comment the gate

Put a one-line WHY on every `#[cfg]`, framed by the target being *excluded* rather than by naming an included platform (the excluded set usually spans several targets):

```rust
#[cfg(not(target_arch = "wasm32"))] // reads the local filesystem
```

Don't gloss `wasm32` as "web"/"browser", and avoid "native-only"/"host-only" as a label for "everything except wasm32".

## When a submodule is overkill

For a single target-only function whose only extra need is one import, a module-top `#[cfg]`-ed `use` plus the `#[cfg]` on that one function is proportional; don't wrap one function in a submodule. Reach for the submodule when a *block* of items (a type, its impls, several functions, multiple scoped imports) shares the same gate.

## Wrong

```rust
#[cfg(feature = "hydrate")]
use base64::Engine;
#[cfg(feature = "hydrate")]
struct LocalStorageStore;
#[cfg(feature = "hydrate")]
impl SettingsStore for LocalStorageStore { /* ... */ }
#[cfg(feature = "hydrate")]
fn local_storage() -> Option<web_sys::Storage> { /* ... */ }
// ...a dozen more, each re-stamping the same #[cfg]
```

## Rationale

Repeating `#[cfg(...)]` on every item is noise that scales with the number of gated items and is easy to get subtly wrong (one item mis-gated). A single gate on the module makes the boundary obvious, scopes the target-specific `use` statements to one place, and keeps the implementation body readable.
