#![cfg(feature = "ssr")]

use leptos::prelude::LeptosOptions;

/* The document the static deploy serves for `/`. Rendered here rather than read from a built tree so the
   assertions hold without a build, which also keeps them independent of whichever build ran last. */
async fn prerendered_document() -> String {
    let leptos_options: LeptosOptions = web::server::read_leptos_manifest_options()
        .expect("the manifest carries the leptos settings");
    let document_bytes = web::server::prerender_document(leptos_options)
        .await
        .expect("the shell route renders");

    String::from_utf8(document_bytes.to_vec())
        .expect("the rendered document is utf-8")
}

#[tokio::test]
async fn prerendered_document_renders_the_map_view_and_its_canvas() {
    let document: String = prerendered_document().await;

    assert!(document.contains(r#"<main id="map-view">"#));
    assert!(document.contains(r#"<canvas id="map-canvas">"#));
}

#[tokio::test]
async fn prerendered_document_references_the_client_bundle() {
    let document: String = prerendered_document().await;

    assert!(document.contains("/pkg/"));
    assert!(document.contains(".wasm"));
    assert!(document.contains(".css"));
}

#[tokio::test]
async fn prerendered_document_names_the_product_in_the_tab() {
    let document: String = prerendered_document().await;

    /* Without it a browser labels the tab with the host, which is how the first deploy read. */
    assert!(document.contains("<title>Eafora</title>"));
}

#[tokio::test]
async fn prerendered_document_references_the_favicon() {
    let document: String = prerendered_document().await;

    /* Without this a browser requests /favicon.ico, gets the 404 the deploy correctly returns for an
       unmatched path, and falls back to its own glyph. The raster copy covers browsers that do not take
       the svg. */
    assert!(document.contains(r#"href="/favicon.svg""#));
    assert!(document.contains(r#"href="/favicon-32.png""#));
}

#[tokio::test]
async fn prerendered_document_declares_one_language_and_direction() {
    let document: String = prerendered_document().await;
    let language_attribute_count: usize = document.matches("lang=").count();
    let direction_attribute_count: usize = document.matches("dir=").count();

    /* A second lang= is a duplicate attribute the parser silently drops, which happened when the shell
       hardcoded one alongside the one the i18n context sets. */
    assert_eq!(language_attribute_count, 1);
    assert_eq!(direction_attribute_count, 1);
}

#[tokio::test]
async fn prerendered_document_omits_the_dev_reload_socket() {
    let document: String = prerendered_document().await;

    /* cargo-leptos sets LEPTOS_WATCH, and the reload script it gates opens a websocket to a developer's
       reload port, which a deployed document must not carry. */
    assert!(!document.contains("ws://"));
    assert!(!document.contains("wss://"));
    assert!(!document.contains("reload_port"));
}
