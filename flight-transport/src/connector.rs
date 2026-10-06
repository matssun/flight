// SPDX-License-Identifier: MIT

use crate::TransportError;
use flight_trust::{client_config, Fingerprint, Identity};
use hyper_util::rt::TokioIo;
use rustls::pki_types::ServerName;
use std::io;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;

/// An HTTP/2 channel to `address` (`host:port`) over mutual TLS 1.3. The server is accepted
/// only if its key fingerprint is `expected`; nothing is sent over a connection to any other.
pub(crate) async fn connect(
    address: &str,
    identity: &Identity,
    expected: &Fingerprint,
) -> Result<Channel, TransportError> {
    let connector = TlsConnector::from(Arc::new(client_config(identity, expected)?));
    let address = address.to_owned();
    // The URI is never dialled: the connector below owns the connection.
    let endpoint = Endpoint::from_static("http://flight.invalid");
    endpoint
        .connect_with_connector(service_fn(move |_: Uri| {
            let (connector, address) = (connector.clone(), address.clone());
            async move {
                let tcp = TcpStream::connect(&address).await?;
                tcp.set_nodelay(true)?;
                let name = ServerName::try_from("flight")
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
                let tls = connector.connect(name, tcp).await?;
                Ok::<_, io::Error>(TokioIo::new(tls))
            }
        }))
        .await
        .map_err(|e| TransportError::Connect(format!("{e:?}")))
}
