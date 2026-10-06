// SPDX-License-Identifier: MIT

use crate::PeerIdentity;
use flight_trust::peer_fingerprint;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::server::Connected;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// A TLS connection whose peer has proved possession of its key.
pub struct TlsConn {
    inner: TlsStream<TcpStream>,
    peer: PeerIdentity,
}

impl Connected for TlsConn {
    type ConnectInfo = PeerIdentity;

    fn connect_info(&self) -> PeerIdentity {
        self.peer.clone()
    }
}

impl AsyncRead for TlsConn {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for TlsConn {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Accept TCP connections, run the TLS handshake for each in its own task (a stalled peer
/// never blocks the others), and yield only connections whose peer authenticated. Ends when
/// `shutdown` flips.
pub(crate) fn accept_tls(
    listener: TcpListener,
    config: Arc<rustls::ServerConfig>,
    mut shutdown: watch::Receiver<bool>,
) -> ReceiverStream<io::Result<TlsConn>> {
    let (tx, rx) = mpsc::channel(32);
    let acceptor = TlsAcceptor::from(config);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                accepted = listener.accept() => {
                    let Ok((tcp, _)) = accepted else { continue };
                    let (acceptor, tx) = (acceptor.clone(), tx.clone());
                    tokio::spawn(async move {
                        let _ = tcp.set_nodelay(true);
                        let handshake = tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(tcp)).await;
                        let Ok(Ok(tls)) = handshake else { return };
                        let Ok(fingerprint) = peer_fingerprint(tls.get_ref().1.peer_certificates()) else {
                            return;
                        };
                        let conn = TlsConn { inner: tls, peer: PeerIdentity(fingerprint) };
                        let _ = tx.send(Ok(conn)).await;
                    });
                }
            }
        }
    });
    ReceiverStream::new(rx)
}
