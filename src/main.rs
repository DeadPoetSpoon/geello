#[cfg(feature = "server")]
#[tokio::main]
async fn main() {
    geello::server::run().await;
}

#[cfg(not(feature = "server"))]
fn main() {
    println!("Hello from Geello!");
}
