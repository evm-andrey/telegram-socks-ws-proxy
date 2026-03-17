#[cfg(unix)]
pub async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    use tokio::time::{sleep, Duration};

    let mut term = signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
    let mut int = signal(SignalKind::interrupt()).expect("failed to register SIGINT handler");

    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }

    sleep(Duration::from_millis(200)).await;
}

#[cfg(not(unix))]
pub async fn wait_for_shutdown_signal() {
    use tokio::signal::ctrl_c;
    let _ = ctrl_c().await;
}
