/* Rendering the component tree monomorphizes deeply nested view types, which overflows the default
   limit while computing their layout. The lib target raises it for the same reason. */
#![recursion_limit = "512"]

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    web::server::run()
        .await;
}

#[cfg(not(feature = "ssr"))]
fn main() {}
