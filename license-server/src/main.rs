#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().any(|a| a == "--healthcheck") {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect_timeout(
            &"127.0.0.1:8080".parse()?,
            std::time::Duration::from_secs(3),
        )?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
        stream
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
        let mut reply = [0; 128];
        let n = stream.read(&mut reply)?;
        anyhow::ensure!(
            reply[..n].starts_with(b"HTTP/1.1 200"),
            "healthcheck failed"
        );
        return Ok(());
    }

    let dir = std::env::var("DATA_DIR").unwrap_or("/data".into());
    let app = assets_license_server::App::open_daily(std::path::Path::new(&dir))?;
    if std::env::args().any(|arg| arg == "--today-token") {
        println!("{}", app.current_daily_token()?);
        return Ok(());
    }
    app.current_daily_token()?;
    let daily = app.clone();
    tokio::spawn(async move {
        loop {
            let seconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            tokio::time::sleep(std::time::Duration::from_secs(
                86400 - (seconds + 14400) % 86400,
            ))
            .await;
            if daily.current_daily_token().is_err() {
                eprintln!("Daily administrator token refresh failed");
            }
        }
    });
    let addr = std::env::var("LISTEN").unwrap_or("0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("License manager listening on {addr}");
    axum::serve(listener, assets_license_server::router(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
