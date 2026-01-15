use crate::robot::state::SharedRobotState;
use tokio::net::UdpSocket;
use tracing::{error, info};

pub async fn run_discovery_service(robot_state: SharedRobotState) {
    let socket = match UdpSocket::bind("0.0.0.0:3001").await {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to bind UDP discovery socket: {}", e);
            return;
        }
    };
    socket.set_broadcast(true).unwrap();

    info!("UDP Discovery service listening on 0.0.0.0:3001");

    let mut buf = [0u8; 1024];

    loop {
        match socket.recv_from(&mut buf).await {
            Ok((size, addr)) => {
                let data = &buf[..size];
                if let Ok(msg) = std::str::from_utf8(data) {
                    info!("Received UDP broadcast from {}: {}", addr, msg);

                    // Expect format: "ROBOT_ANNOUNCE:PORT" or JSON like {"port": 8000}
                    // For simplicity, let's assume JSON: {"type": "announce", "port": 8000}

                    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(msg) {
                        if payload["type"] == "announce" {
                            if let Some(port) = payload["port"].as_u64() {
                                let ip = addr.ip();
                                let url = format!("http://{}:{}", ip, port);

                                {
                                    let mut url_lock = robot_state.robot_url.write().await;
                                    if url_lock.as_deref() != Some(&url) {
                                        info!("Registered robot at {}", url);
                                        *url_lock = Some(url);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!("Error receiving UDP packet: {}", e);
            }
        }
    }
}
