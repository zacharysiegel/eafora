#![cfg(feature = "ssr")]

use leptos::prelude::LeptosOptions;

/* The document the static deploy serves for `/`. Rendered here rather than read from a built tree so the
   assertions hold without a build, which also keeps them independent of whichever build ran last. */
async fn render_shell_document() -> String {
    let leptos_options: LeptosOptions = web::server::read_manifest_options();
    let document_bytes = web::server::render_shell_document(leptos_options)
        .await;

    String::from_utf8(document_bytes.to_vec())
        .expect("the rendered document is utf-8")
}

#[tokio::test]
async fn render_shell_document_mounts_the_app_on_its_own_container() {
    let document: String = render_shell_document().await;

    /* Mounting on <body> would entangle Leptos's reactive tree with extension-injected nodes. */
    assert!(document.contains(r#"<div id="leptos">"#) || document.contains(r#"id="map-view""#));
    assert!(document.contains("<canvas"));
}

#[tokio::test]
async fn render_shell_document_references_the_client_bundle() {
    let document: String = render_shell_document().await;

    assert!(document.contains("/pkg/"));
    assert!(document.contains(".wasm"));
    assert!(document.contains(".css"));
}

#[tokio::test]
async fn render_shell_document_declares_one_language_and_direction() {
    let document: String = render_shell_document().await;
    let language_attribute_count: usize = document.matches("lang=").count();
    let direction_attribute_count: usize = document.matches("dir=").count();

    /* A second lang= is a duplicate attribute the parser silently drops, which happened when the shell
       hardcoded one alongside the one the i18n context sets. */
    assert_eq!(language_attribute_count, 1);
    assert_eq!(direction_attribute_count, 1);
}

#[tokio::test]
async fn render_shell_document_omits_the_dev_reload_script() {
    let document: String = render_shell_document().await;

    /* cargo-leptos sets LEPTOS_WATCH, and a deployed document carrying a websocket pointing at a
       developer's reload port would be inert at best. */
    assert!(!document.contains("__leptos_hot_reload"));
    assert!(!document.contains("ws://"));
}
