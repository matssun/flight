// SPDX-License-Identifier: MIT

use crate::TransportError;
use flight_trust::{client_config, Fingerprint, Identity};
use hyper_util::rt::TokioIo;
use rustls::pki_types::ServerName;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;

/// Upper bound for each step of dialling (TCP connect, TLS handshake).
pub const DEFAULT_DIAL_TIMEOUT: Duration = Duration::from_secs(5);
/// HTTP/2 PING cadence and tolerance on the client: a half-open connection is detected.
const KEEPALIVE: Duration = Duration::from_secs(10);
/// What each stream and the whole connection may have in flight toward this end before the
/// sender waits. Streams of a kept connection share its window: a stream nobody reads holds up
/// to a stream window of it, so the connection window is large enough that several stalled
/// streams cannot leave a healthy one with nothing.
pub(crate) const STREAM_WINDOW: u32 = 64 * 1024;
pub(crate) const CONNECTION_WINDOW: u32 = 2 * 1024 * 1024;

/// An HTTP/2 channel to `address` (`host:port`) over mutual TLS 1.3. The server is accepted
/// only if its key fingerprint is `expected`; nothing is sent over a connection to any other.
pub(crate) async fn connect(
    address: &str,
    identity: &Identity,
    expected: &Fingerprint,
) -> Result<Channel, TransportError> {
    connect_with(address, identity, expected, DEFAULT_DIAL_TIMEOUT).await
}

/// [`connect`] with an explicit bound on each dialling step. Nothing here waits forever.
pub(crate) async fn connect_with(
    address: &str,
    identity: &Identity,
    expected: &Fingerprint,
    dial_timeout: Duration,
) -> Result<Channel, TransportError> {
    let connector = TlsConnector::from(Arc::new(client_config(identity, expected)?));
    let address = address.to_owned();
    let unreachable = Arc::new(AtomicBool::new(false));
    let flag = unreachable.clone();
    // The URI is never dialled: the connector below owns the connection.
    let endpoint = Endpoint::from_static("http://flight.invalid")
        .connect_timeout(dial_timeout * 2 + Duration::from_secs(1))
        .http2_keep_alive_interval(KEEPALIVE)
        .keep_alive_timeout(KEEPALIVE)
        .keep_alive_while_idle(true)
        .initial_stream_window_size(STREAM_WINDOW)
        .initial_connection_window_size(CONNECTION_WINDOW);
    endpoint
        .connect_with_connector(service_fn(move |_: Uri| {
            let (connector, address, flag) = (connector.clone(), address.clone(), flag.clone());
            async move {
                let tcp = match tokio::time::timeout(dial_timeout, TcpStream::connect(&address))
                    .await
                {
                    Ok(Ok(tcp)) => tcp,
                    Ok(Err(e)) => {
                        if is_unreachable_kind(e.kind()) {
                            flag.store(true, Ordering::Relaxed);
                        }
                        return Err(e);
                    }
                    Err(_) => return Err(io::Error::new(io::ErrorKind::TimedOut, "tcp connect")),
                };
                tcp.set_nodelay(true)?;
                let name = ServerName::try_from("flight")
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
                let tls = tokio::time::timeout(dial_timeout, connector.connect(name, tcp))
                    .await
                    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "tls handshake"))??;
                Ok::<_, io::Error>(TokioIo::new(tls))
            }
        }))
        .await
        .map_err(|e| {
            let text = format!("{e:?}");
            if unreachable.load(Ordering::Relaxed) {
                TransportError::Unreachable(text)
            } else {
                TransportError::Connect(text)
            }
        })
}

/// The OS refused to route to the peer (as opposed to the peer being down).
pub(crate) fn is_unreachable_kind(kind: io::ErrorKind) -> bool {
    matches!(
        kind,
        io::ErrorKind::HostUnreachable | io::ErrorKind::NetworkUnreachable
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_routing_failures_count_as_unreachable() {
        assert!(is_unreachable_kind(io::ErrorKind::HostUnreachable));
        assert!(is_unreachable_kind(io::ErrorKind::NetworkUnreachable));
        for k in [
            io::ErrorKind::ConnectionRefused,
            io::ErrorKind::TimedOut,
            io::ErrorKind::ConnectionReset,
            io::ErrorKind::NotFound,
        ] {
            assert!(!is_unreachable_kind(k), "{k:?}");
        }
    }
}
