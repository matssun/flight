// SPDX-License-Identifier: MIT

//! Mutual TLS by fingerprint, driven in memory (no sockets): who is accepted, who is
//! refused, and who the server believes the client is.

use flight_trust::{client_config, peer_fingerprint, server_config, Fingerprint, Identity};
use rustls::pki_types::ServerName;
use rustls::{ClientConnection, ServerConnection};
use std::io::Cursor;
use std::sync::Arc;

struct Outcome {
    client_error: Option<String>,
    server_error: Option<String>,
    server_saw: Option<Fingerprint>,
}

fn handshake(client: &Identity, server: &Identity, client_expects: &Fingerprint) -> Outcome {
    let c_cfg = client_config(client, client_expects).expect("client config");
    let s_cfg = server_config(server).expect("server config");
    let name = ServerName::try_from("flight").expect("name");
    let mut c = ClientConnection::new(Arc::new(c_cfg), name).expect("client");
    let mut s = ServerConnection::new(Arc::new(s_cfg)).expect("server");
    let (mut client_error, mut server_error) = (None, None);
    for _ in 0..20 {
        let mut buf = Vec::new();
        while c.wants_write() {
            c.write_tls(&mut buf).expect("write");
        }
        if !buf.is_empty() {
            let mut cur = Cursor::new(buf);
            if s.read_tls(&mut cur).is_ok() {
                if let Err(e) = s.process_new_packets() {
                    server_error = Some(e.to_string());
                }
            }
        }
        let mut buf = Vec::new();
        while s.wants_write() {
            s.write_tls(&mut buf).expect("write");
        }
        if !buf.is_empty() {
            let mut cur = Cursor::new(buf);
            if c.read_tls(&mut cur).is_ok() {
                if let Err(e) = c.process_new_packets() {
                    client_error = Some(e.to_string());
                }
            }
        }
        if client_error.is_some()
            || server_error.is_some()
            || (!c.is_handshaking() && !s.is_handshaking())
        {
            break;
        }
    }
    let server_saw = if server_error.is_none() && client_error.is_none() {
        peer_fingerprint(s.peer_certificates()).ok()
    } else {
        None
    };
    Outcome {
        client_error,
        server_error,
        server_saw,
    }
}

#[test]
fn pinned_server_and_any_client_complete_and_the_server_learns_the_clients_id() {
    let (node, orch) = (
        Identity::generate().expect("n"),
        Identity::generate().expect("o"),
    );
    let out = handshake(&node, &orch, orch.fingerprint());
    assert!(
        out.client_error.is_none() && out.server_error.is_none(),
        "{:?} {:?}",
        out.client_error,
        out.server_error
    );
    assert_eq!(out.server_saw.as_ref(), Some(node.fingerprint()));
}

#[test]
fn a_node_refuses_a_server_with_the_wrong_fingerprint() {
    let (node, orch, impostor) = (
        Identity::generate().expect("n"),
        Identity::generate().expect("o"),
        Identity::generate().expect("i"),
    );
    // The node expects `orch` but reaches `impostor`: it must refuse before sending anything
    // (such as an enrollment token) over the connection.
    let out = handshake(&node, &impostor, orch.fingerprint());
    assert!(out.client_error.is_some());
    assert!(out.server_saw.is_none());
}

#[test]
fn two_clients_with_the_same_display_name_are_distinct_identities() {
    let orch = Identity::generate().expect("o");
    let (a, b) = (
        Identity::generate().expect("a"),
        Identity::generate().expect("b"),
    );
    let seen_a = handshake(&a, &orch, orch.fingerprint()).server_saw;
    let seen_b = handshake(&b, &orch, orch.fingerprint()).server_saw;
    assert_ne!(seen_a, seen_b);
}

#[test]
fn a_client_without_a_certificate_is_refused_by_the_server() {
    // Build a client config with no client auth by hand.
    let orch = Identity::generate().expect("o");
    let s_cfg = server_config(&orch).expect("server");
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let c_cfg = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAll))
        .with_no_client_auth();
    let mut c = ClientConnection::new(Arc::new(c_cfg), ServerName::try_from("flight").expect("n"))
        .expect("c");
    let mut s = ServerConnection::new(Arc::new(s_cfg)).expect("s");
    let mut failed = false;
    for _ in 0..20 {
        let mut buf = Vec::new();
        while c.wants_write() {
            c.write_tls(&mut buf).expect("w");
        }
        if !buf.is_empty() {
            s.read_tls(&mut Cursor::new(buf)).expect("r");
            failed |= s.process_new_packets().is_err();
        }
        let mut buf = Vec::new();
        while s.wants_write() {
            s.write_tls(&mut buf).expect("w");
        }
        if !buf.is_empty() {
            c.read_tls(&mut Cursor::new(buf)).expect("r");
            failed |= c.process_new_packets().is_err();
        }
        if failed {
            break;
        }
    }
    assert!(failed, "mutual TLS must require a client certificate");
}

#[derive(Debug)]
struct AcceptAll;

impl rustls::client::danger::ServerCertVerifier for AcceptAll {
    fn verify_server_cert(
        &self,
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
