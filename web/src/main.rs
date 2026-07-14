// The SSR-build binary cargo-leptos compiles with the `ssr` feature and runs as the dev server
// (`cargo leptos watch`). Production is served as static assets from Cloudflare Workers Assets; the
// static-export step that pre-renders the routes for that deploy is wired in the deploy phase.
#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    use std::net::SocketAddr;

    use axum::Router;
    use leptos::logging::log;
    use leptos::prelude::*;
    use leptos_axum::LeptosRoutes;

    use web::app::{shell, App};

    let configuration = get_configuration(None)
        .unwrap();
    let leptos_options: LeptosOptions = configuration.leptos_options;
    let address: SocketAddr = leptos_options.site_addr;
    let route_listings = leptos_axum::generate_route_list(App);

    let router: Router = Router::new()
        .leptos_routes(&leptos_options, route_listings, {
            let leptos_options: LeptosOptions = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options);

    let listener: tokio::net::TcpListener = tokio::net::TcpListener::bind(&address)
        .await
        .unwrap();
    log!("listening on http://{}", &address);

    axum::serve(listener, router.into_make_service())
        .await
        .unwrap();
}

#[cfg(not(feature = "ssr"))]
fn main() {}
