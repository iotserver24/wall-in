use clap::{Parser, Subcommand};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};
use wall_in_core::{
    platform::workspace_root,
    service::{Service, router, supervise},
    store::default_data_dir,
};

#[derive(Parser)]
#[command(
    name = "wall-in",
    version,
    about = "Local Linux wallpaper library and renderer service"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Serve the local wallpaper manager and JSON API.
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: IpAddr,
        #[arg(long, default_value_t = 3977)]
        port: u16,
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
}
#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("wall-in: {error}");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Command::Serve {
        host,
        port,
        data_dir,
    } = Cli::parse().command;
    if !host.is_loopback() {
        return Err("Wall-in only accepts loopback bind addresses".into());
    }
    run_server(
        host,
        port,
        data_dir.map(Ok).unwrap_or_else(default_data_dir)?,
        workspace_root(),
    )
    .await
}
async fn run_server(
    host: IpAddr,
    port: u16,
    data_dir: PathBuf,
    root: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::new(host, port)).await?;
    let shared = Service::open(data_dir, root.join("integrations/renderer.py")).await?;
    let address = listener.local_addr()?;
    let app = router(
        shared.clone(),
        root.join("apps/desktop/dist"),
        address.port(),
    );
    let monitor = supervise(&shared);
    eprintln!("Wall-in listening at http://{address}");
    let result = axum_serve(listener, app).await;
    monitor.abort();
    shared.lock().await.shutdown().await;
    result?;
    Ok(())
}
async fn axum_serve(
    listener: tokio::net::TcpListener,
    app: wall_in_core::service::HttpRouter,
) -> std::io::Result<()> {
    wall_in_core::service::serve(listener, app, shutdown_signal()).await
}
async fn shutdown_signal() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn occupied_port_fails_before_opening_library_or_restoring() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let path = std::env::temp_dir().join(format!(
            "wall-in-bind-test-{}-{}",
            std::process::id(),
            address.port()
        ));
        assert!(!path.exists());
        let error = run_server(address.ip(), address.port(), path.clone(), path.clone())
            .await
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::AddrInUse
        );
        assert!(
            !path.exists(),
            "service opened its data directory before binding"
        );
    }
}
