#![forbid(unsafe_code)]

fn production_options(
    resolver: std::sync::Arc<ck_cingulate::EntorhinalResolver>,
) -> ck_cingulate::Options {
    ck_cingulate::Options {
        resolver,
        ..Default::default()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Consume the supervisor nonce before any application background tasks.
    let _nonce = subc_client_rs::launch_nonce()?;
    // The daemon captures stderr as this module's log. Without a subscriber every
    // tracing event, including the one line logged per refusal, is dropped.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .init();
    let resolver = std::sync::Arc::new(ck_cingulate::EntorhinalResolver::default());
    let handler = ck_cingulate::Handler::new(production_options(resolver.clone()));
    let (module, serving) =
        subc_client_rs::serve_from_env_with_handle(ck_cingulate::manifest(), handler.clone())
            .await?;
    let runtime = std::sync::Arc::new(ck_cingulate::DaemonAccessRuntime::new(module));
    handler.set_access_runtime(runtime.clone());
    resolver.set_runtime(runtime.clone());
    handler.set_permissions_runtime(runtime);
    serving.await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn production_uses_entorhinal_resolver() {
        let resolver = Arc::new(ck_cingulate::EntorhinalResolver::default());
        let expected: Arc<dyn ck_cingulate::Resolver> = resolver.clone();
        assert!(Arc::ptr_eq(
            &production_options(resolver).resolver,
            &expected
        ));
    }
}
