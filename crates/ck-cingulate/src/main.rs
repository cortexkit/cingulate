#![forbid(unsafe_code)]

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Consume the supervisor nonce before any application background tasks.
    let _nonce = subc_client_rs::launch_nonce()?;
    let handler = ck_cingulate::Handler::new(ck_cingulate::Options::default());
    let (module, serving) =
        subc_client_rs::serve_from_env_with_handle(ck_cingulate::manifest(), handler.clone())
            .await?;
    handler.set_access_runtime(std::sync::Arc::new(ck_cingulate::DaemonAccessRuntime::new(
        module,
    )));
    serving.await?;
    Ok(())
}
