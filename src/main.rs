#[cfg(feature = "server")]
mod server;
#[cfg(feature = "server")]
#[macro_use]
extern crate rocket;
#[cfg(feature = "server")]
#[launch]
async fn rocket() -> _ {
    server::rocket().await
}
