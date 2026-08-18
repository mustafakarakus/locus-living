#[tokio::main]
async fn main() {
    if let Err(err) = homeai_core::run().await {
        eprintln!("homeai-core failed: {err:#}");
        std::process::exit(1);
    }
}
