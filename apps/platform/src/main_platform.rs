#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() {
    if let Err(err) = platform::server::run().await {
        tracing::error!(?err, "server exited with error");
        std::process::exit(1);
    }
}

#[cfg(not(feature = "ssr"))]
fn main() {}
