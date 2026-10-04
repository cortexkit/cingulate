#![forbid(unsafe_code)]

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Consume the supervisor nonce before any application background tasks.
    let _nonce = subc_client_rs::launch_nonce()?;
    subc_client_rs::serve(
        ck_cingulate::manifest(),
        ck_cingulate::Handler::new(ck_cingulate::Options::default()),
    )
    .await?;
    Ok(())
}
