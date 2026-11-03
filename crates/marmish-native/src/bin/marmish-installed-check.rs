#[tokio::main]
async fn main() {
    std::process::exit(marmish_native::installed_check_entry().await.into());
}
