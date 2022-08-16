pub fn try_init_tracing_subscriber() -> Result<(), Box<dyn std::error::Error>> {
    let builder = tracing_subscriber::fmt::Subscriber::builder();
    let env_filter = std::env::var(tracing_subscriber::EnvFilter::DEFAULT_ENV)
        .map(tracing_subscriber::EnvFilter::new)
        .unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(format!(
                "{}={}",
                env!("CARGO_PKG_NAME").replace('-', "_"),
                tracing::Level::WARN
            ))
        });
    let builder = builder.with_env_filter(env_filter);
    let subscriber = builder.finish();
    use tracing_subscriber::util::SubscriberInitExt;
    subscriber.try_init()?;
    Ok(())
}
