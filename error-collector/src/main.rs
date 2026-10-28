use actix_web::{App, HttpServer, web};
use std::{path::PathBuf, sync::Mutex};
use wega_error_collector::{MAX_BODY, State, error_auth, routes, store::Store};
#[actix_web::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("healthcheck") {
        use std::io::{Read, Write};
        let mut address: std::net::SocketAddr = std::env::var("BIND_ADDRESS")
            .unwrap_or_else(|_| "0.0.0.0:7090".into())
            .parse()?;
        if address.ip().is_unspecified() {
            address.set_ip(if address.is_ipv4() {
                std::net::Ipv4Addr::LOCALHOST.into()
            } else {
                std::net::Ipv6Addr::LOCALHOST.into()
            });
        }
        let timeout = std::time::Duration::from_secs(2);
        let mut stream = std::net::TcpStream::connect_timeout(&address, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.write_all(b"GET /healthz HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut status = [0; 12];
        stream.read_exact(&mut status)?;
        if !status.starts_with(b"HTTP/1.0 200") && !status.starts_with(b"HTTP/1.1 200") {
            return Err("Collector health check failed".into());
        }
        return Ok(());
    }
    let directory = PathBuf::from(std::env::var_os("DATA_DIR").unwrap_or_else(|| "/data".into()));
    std::fs::create_dir_all(&directory)?;
    let days = std::env::var("RETENTION_DAYS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let store = Store::open(&directory.join("errors.sqlite"), days)?;
    match args.first().map(String::as_str) {
        Some("enroll") if args.len()==3=>{error_auth::unhex::<16>(&args[1])?;store.enroll(&args[1],&error_auth::unhex::<32>(&args[2])?)?;println!("Client enrolled: {}",args[1]);return Ok(());}
        Some("revoke") if args.len()==2=>{store.revoke(&args[1])?;println!("Client revoked: {}",args[1]);return Ok(());}
        Some("export")=>{for event in store.export(args.get(1).and_then(|n|n.parse().ok()).unwrap_or(100))?{println!("{event}");}return Ok(());}
        Some(_)=>return Err("Usage: wega-error-collector [enroll CLIENT_ID PUBLIC_KEY | revoke CLIENT_ID | export LIMIT]".into()),
        None=>{}
    }
    let bind = std::env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:7090".into());
    let state = web::Data::new(State {
        store: Mutex::new(store),
    });
    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .app_data(web::PayloadConfig::new(MAX_BODY))
            .configure(routes::configure)
    })
    .workers(2)
    .client_request_timeout(std::time::Duration::from_secs(10))
    .bind(bind)?
    .run()
    .await?;
    Ok(())
}
