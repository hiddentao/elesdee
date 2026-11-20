//! Phone-side demo-server forwarding.
//!
//! The shell binds a loopback listener for each registered demo-server port and
//! bridges every accepted socket to the paired computer over the forward ALPN.
//! The listener uses `127.0.0.1` only, so no other device can reach it.
//!
//! A port gets one tunnel. When the preferred port is taken, the shell binds a
//! free port and shows that address instead. The phone holds at most the shared
//! tunnel limit across all sessions.

use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use elesdee_link::{
    ForwardConnection, ForwardTunnel, MAX_FORWARD_TUNNELS, phone_address, port_allowed,
};
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// A demo-server tunnel failure that the screens can explain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardError {
    /// The port is outside the demo-server range.
    InvalidPort,
    /// The port is not in the computer's active demo-server list.
    NotRegistered,
    /// Another tunnel already uses the port.
    PortInUse,
    /// The phone holds the maximum number of tunnels.
    Limit,
    /// The loopback listener could not bind.
    Bind(String),
    /// The forward connection is closed.
    Closed,
}

impl ForwardError {
    /// Returns a stable code for the interface.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidPort => "invalid_port",
            Self::NotRegistered => "not_registered",
            Self::PortInUse => "port_in_use",
            Self::Limit => "limit",
            Self::Bind(_) => "bind",
            Self::Closed => "connection_closed",
        }
    }
}

/// The loopback address that a tunnel exposes on the phone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardAddress {
    /// The demo-server port on the computer.
    pub port: u16,
    /// The port that the phone listener uses.
    pub local_port: u16,
    /// The address to open on the phone, such as `http://127.0.0.1:3000`.
    pub address: String,
}

/// Checks a requested port against the active demo-server list.
///
/// # Errors
///
/// Refuses a privileged or empty port and any port that no session registered.
pub fn check_port(port: u16, registered: &HashSet<u16>) -> Result<(), ForwardError> {
    if !port_allowed(port) {
        return Err(ForwardError::InvalidPort);
    }
    if !registered.contains(&port) {
        return Err(ForwardError::NotRegistered);
    }
    Ok(())
}

/// The open tunnels of one shell process.
#[derive(Clone, Default)]
pub struct ForwardRegistry {
    inner: Arc<Mutex<RegistryInner>>,
}

impl ForwardRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens a tunnel for a registered demo-server port.
    ///
    /// # Errors
    ///
    /// Refuses a port outside the registered list, a duplicate port, a ninth
    /// tunnel and a listener that cannot bind.
    pub async fn start(
        &self,
        computer_id: &str,
        port: u16,
        registered: &HashSet<u16>,
        forward: ForwardConnection,
    ) -> Result<ForwardAddress, ForwardError> {
        check_port(port, registered)?;
        self.prune();
        {
            let inner = self.lock();
            if inner.tunnels.contains_key(&port) {
                return Err(ForwardError::PortInUse);
            }
            if inner.tunnels.len() >= MAX_FORWARD_TUNNELS {
                return Err(ForwardError::Limit);
            }
        }
        if forward.is_closed() {
            return Err(ForwardError::Closed);
        }

        let (listener, local_port) = bind_local(port)
            .await
            .map_err(|error| ForwardError::Bind(error.to_string()))?;
        let address = phone_address(local_port);

        let (cancel, cancel_rx) = watch::channel(false);
        let mut inner = self.lock();
        // Re-check after the bind wait, so a concurrent start cannot exceed a
        // limit.
        if inner.tunnels.contains_key(&port) {
            return Err(ForwardError::PortInUse);
        }
        if inner.tunnels.len() >= MAX_FORWARD_TUNNELS {
            return Err(ForwardError::Limit);
        }
        let task_forward = forward.clone();
        let task = tokio::spawn(accept_loop(listener, task_forward, port, cancel_rx));
        inner.tunnels.insert(
            port,
            Tunnel {
                computer_id: computer_id.to_owned(),
                local_port,
                address: address.clone(),
                cancel,
                forward,
                task,
            },
        );
        Ok(ForwardAddress {
            port,
            local_port,
            address,
        })
    }

    /// Returns the address for an open tunnel on one computer.
    pub fn address(&self, computer_id: &str, port: u16) -> Option<ForwardAddress> {
        self.prune();
        self.lock()
            .tunnels
            .get(&port)
            .filter(|tunnel| tunnel.computer_id == computer_id)
            .map(|tunnel| ForwardAddress {
                port,
                local_port: tunnel.local_port,
                address: tunnel.address.clone(),
            })
    }

    /// Stops one tunnel. Returns false when no tunnel uses the port.
    pub fn stop(&self, port: u16) -> bool {
        let tunnel = self.lock().tunnels.remove(&port);
        match tunnel {
            Some(tunnel) => {
                tunnel.shutdown();
                true
            }
            None => false,
        }
    }

    /// Stops every tunnel for one computer.
    pub fn stop_for_computer(&self, computer_id: &str) {
        let ports = {
            let inner = self.lock();
            inner
                .tunnels
                .iter()
                .filter(|(_, tunnel)| tunnel.computer_id == computer_id)
                .map(|(port, _)| *port)
                .collect::<Vec<_>>()
        };
        for port in ports {
            self.stop(port);
        }
    }

    /// Stops every tunnel.
    pub fn close_all(&self) {
        let tunnels = {
            let mut inner = self.lock();
            std::mem::take(&mut inner.tunnels)
        };
        for tunnel in tunnels.into_values() {
            tunnel.shutdown();
        }
    }

    fn prune(&self) {
        let dead = {
            let inner = self.lock();
            inner
                .tunnels
                .iter()
                .filter(|(_, tunnel)| tunnel.forward.is_closed())
                .map(|(port, _)| *port)
                .collect::<Vec<_>>()
        };
        for port in dead {
            self.stop(port);
        }
    }

    fn lock(&self) -> MutexGuard<'_, RegistryInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[derive(Default)]
struct RegistryInner {
    tunnels: HashMap<u16, Tunnel>,
}

struct Tunnel {
    computer_id: String,
    local_port: u16,
    address: String,
    cancel: watch::Sender<bool>,
    forward: ForwardConnection,
    task: JoinHandle<()>,
}

impl Tunnel {
    fn shutdown(self) {
        let _ = self.cancel.send(true);
        self.forward.close();
        self.task.abort();
    }
}

/// Binds `127.0.0.1` on the preferred port, or on a free port when it is taken.
pub async fn bind_local(preferred: u16) -> std::io::Result<(TcpListener, u16)> {
    let preferred = SocketAddr::from((Ipv4Addr::LOCALHOST, preferred));
    match TcpListener::bind(preferred).await {
        Ok(listener) => Ok((listener, preferred.port())),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
            let port = listener.local_addr()?.port();
            Ok((listener, port))
        }
        Err(error) => Err(error),
    }
}

async fn accept_loop(
    listener: TcpListener,
    forward: ForwardConnection,
    port: u16,
    mut cancel: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    break;
                }
            }
            closed = forward.wait_closed() => {
                tracing::info!(%closed, port, "the demo-server forward connection closed");
                break;
            }
            accepted = listener.accept() => {
                let Ok((socket, _peer)) = accepted else {
                    break;
                };
                let forward = forward.clone();
                tokio::spawn(async move {
                    match forward.open(port).await {
                        Ok(tunnel) => bridge(socket, tunnel).await,
                        Err(error) => {
                            tracing::info!(%error, port, "a demo-server tunnel did not open");
                        }
                    }
                });
            }
        }
    }
}

async fn bridge(socket: TcpStream, tunnel: ForwardTunnel) {
    let _ = socket.set_nodelay(true);
    let (mut tcp_read, mut tcp_write) = socket.into_split();
    let (mut send, mut recv) = tunnel.split();
    let to_socket = async {
        let _ = tokio::io::copy(&mut recv, &mut tcp_write).await;
        let _ = tcp_write.shutdown().await;
    };
    let to_tunnel = async {
        let _ = tokio::io::copy(&mut tcp_read, &mut send).await;
        let _ = send.finish();
    };
    tokio::join!(to_socket, to_tunnel);
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use elesdee_link::{
        ComputerRecord, DeviceIdentity, FORWARD_ALPN, FORWARD_OK, LinkClient, PROTOCOL_VERSION,
        PhoneStore, StoreError,
    };
    use iroh::Endpoint;
    use iroh::endpoint::presets;
    use tokio::io::AsyncReadExt;

    use super::*;

    #[tokio::test]
    async fn a_free_preferred_port_keeps_its_number() {
        let probe = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("probe bind");
        let free = probe.local_addr().expect("probe address").port();
        drop(probe);

        let (listener, port) = bind_local(free).await.expect("bind");
        assert_eq!(port, free);
        drop(listener);
    }

    #[tokio::test]
    async fn an_occupied_port_gets_another_free_port() {
        let held = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("hold bind");
        let taken = held.local_addr().expect("held address").port();

        let (listener, port) = bind_local(taken).await.expect("bind");
        assert_ne!(port, taken);
        assert_ne!(port, 0);
        assert_eq!(
            listener.local_addr().expect("listener address").ip(),
            Ipv4Addr::LOCALHOST
        );
        drop(listener);
        drop(held);
    }

    #[test]
    fn an_unregistered_port_is_refused() {
        let registered = HashSet::from([3000]);
        assert_eq!(check_port(3000, &registered), Ok(()));
        assert_eq!(
            check_port(3001, &registered),
            Err(ForwardError::NotRegistered)
        );
        assert_eq!(
            check_port(80, &HashSet::from([80])),
            Err(ForwardError::InvalidPort)
        );
    }

    struct EmptyStore;

    impl PhoneStore for EmptyStore {
        fn load_identity(&self) -> Result<Option<[u8; 32]>, StoreError> {
            Ok(None)
        }

        fn save_identity(&self, _secret: &[u8; 32]) -> Result<(), StoreError> {
            Ok(())
        }

        fn load_computers(&self) -> Result<Vec<ComputerRecord>, StoreError> {
            Ok(Vec::new())
        }

        fn save_computers(&self, _computers: &[ComputerRecord]) -> Result<(), StoreError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_listener_bridges_a_socket_over_the_forward_alpn() {
        let server = Endpoint::builder(presets::Minimal)
            .alpns(vec![FORWARD_ALPN.to_vec()])
            .bind_addr(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .expect("server bind address")
            .bind()
            .await
            .expect("server binds");
        let server_addr = server.addr();
        let server_id = server.id();

        let server_task = tokio::spawn(async move {
            let incoming = server.accept().await.expect("forward connection");
            let connection = incoming.await.expect("forward completes");
            let (mut send, mut recv) = connection.accept_bi().await.expect("tunnel stream");
            let mut port = [0u8; 2];
            recv.read_exact(&mut port).await.expect("port bytes");
            assert_eq!(u16::from_be_bytes(port), 3000);
            send.write_all(&[FORWARD_OK]).await.expect("status");
            let mut request = [0u8; 4];
            recv.read_exact(&mut request).await.expect("request bytes");
            assert_eq!(&request, b"ping");
            send.write_all(&request).await.expect("echo");
            let _ = connection.closed().await;
            server.close().await;
        });

        let identity = DeviceIdentity::generate();
        let endpoint = Endpoint::builder(presets::Minimal)
            .secret_key(identity.secret_key().clone())
            .alpns(vec![FORWARD_ALPN.to_vec()])
            .bind_addr(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .expect("client bind address")
            .bind()
            .await
            .expect("client binds");
        let client = LinkClient::from_endpoint(Arc::new(EmptyStore), identity, endpoint);
        let computer = ComputerRecord {
            id: server_id.to_z32(),
            name: "Studio computer".to_owned(),
            addr: server_addr,
            protocol_version: PROTOCOL_VERSION,
            min_bridge_version: 1,
            paired_at: 0,
        };
        let forward = client.forward(&computer).await.expect("forward connects");

        let registry = ForwardRegistry::new();
        let address = registry
            .start("computer", 3000, &HashSet::from([3000]), forward)
            .await
            .expect("listener starts");
        assert_eq!(address.address, phone_address(address.local_port));

        let mut socket = TcpStream::connect((Ipv4Addr::LOCALHOST, address.local_port))
            .await
            .expect("local connect");
        socket.write_all(b"ping").await.expect("socket write");
        let mut echo = [0u8; 4];
        socket.read_exact(&mut echo).await.expect("socket read");
        assert_eq!(&echo, b"ping");

        registry.close_all();
        client.shutdown().await;
        server_task.await.expect("server task");
    }
}
