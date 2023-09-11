// Copyright 2021 Parity Technologies (UK) Ltd.
// Copyright 2022 Protocol Labs.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! TLS configuration based on libp2p TLS specs.
//!
//! See <https://github.com/libp2p/specs/blob/master/tls/tls.md>.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

use crate::crypto::ed25519::Keypair;
use crate::PeerId;
use rustls::{ClientConfig, ServerConfig};
use s2n_quic::provider::tls::{
    rustls::{Client as TlsClient, Server as TlsServer},
    Provider,
};
use tokio::sync::mpsc::Sender;

use std::sync::Arc;

pub mod certificate;
mod upgrade;
mod verifier;

// TODO: remove maybe
pub use futures_rustls::TlsStream;
pub use upgrade::UpgradeError;

const P2P_ALPN: [u8; 6] = *b"libp2p";

pub(crate) struct TlsProvider {
    /// Private key.
    private_key: rustls::PrivateKey,

    /// Certificate.
    certificate: rustls::Certificate,

    /// Remote peer ID, provided for the TLS client.
    remote_peer_id: Option<PeerId>,

    /// Sender for the peer ID.
    sender: Option<Sender<PeerId>>,
}

impl TlsProvider {
    /// Create new [`TlsProvider`].
    pub(crate) fn new(
        private_key: rustls::PrivateKey,
        certificate: rustls::Certificate,
        remote_peer_id: Option<PeerId>,
        sender: Option<Sender<PeerId>>,
    ) -> Self {
        Self {
            sender,
            private_key,
            certificate,
            remote_peer_id,
        }
    }
}

impl Provider for TlsProvider {
    type Server = TlsServer;
    type Client = TlsClient;
    type Error = rustls::Error;

    fn start_server(self) -> Result<Self::Server, Self::Error> {
        let mut cfg = ServerConfig::builder()
            .with_cipher_suites(verifier::CIPHERSUITES)
            .with_safe_default_kx_groups()
            .with_protocol_versions(verifier::PROTOCOL_VERSIONS)
            .expect("Cipher suites and kx groups are configured; qed")
            .with_client_cert_verifier(Arc::new(verifier::Libp2pCertificateVerifier::with_sender(
                self.sender,
            )))
            .with_single_cert(vec![self.certificate], self.private_key)
            .expect("Server cert key DER is valid; qed");

        cfg.alpn_protocols = vec![P2P_ALPN.to_vec()];
        Ok(cfg.into())
    }

    fn start_client(self) -> Result<Self::Client, Self::Error> {
        let mut cfg = ClientConfig::builder()
            .with_cipher_suites(verifier::CIPHERSUITES)
            .with_safe_default_kx_groups()
            .with_protocol_versions(verifier::PROTOCOL_VERSIONS)
            .expect("Cipher suites and kx groups are configured; qed")
            .with_custom_certificate_verifier(Arc::new(
                verifier::Libp2pCertificateVerifier::with_remote_peer_id(self.remote_peer_id),
            ))
            .with_single_cert(vec![self.certificate], self.private_key)
            .expect("Client cert key DER is valid; qed");

        cfg.alpn_protocols = vec![P2P_ALPN.to_vec()];
        Ok(cfg.into())
    }
}

/// Create a TLS server configuration for litep2p.
pub fn make_server_config(
    keypair: &Keypair,
) -> Result<rustls::ServerConfig, certificate::GenError> {
    let (certificate, private_key) = certificate::generate(keypair)?;

    let mut crypto = rustls::ServerConfig::builder()
        .with_cipher_suites(verifier::CIPHERSUITES)
        .with_safe_default_kx_groups()
        .with_protocol_versions(verifier::PROTOCOL_VERSIONS)
        .expect("Cipher suites and kx groups are configured; qed")
        .with_client_cert_verifier(Arc::new(verifier::Libp2pCertificateVerifier::new()))
        .with_single_cert(vec![certificate], private_key)
        .expect("Server cert key DER is valid; qed");
    crypto.alpn_protocols = vec![P2P_ALPN.to_vec()];

    Ok(crypto)
}

/// Create a TLS client configuration for libp2p.
pub fn make_client_config(
    keypair: &Keypair,
    remote_peer_id: Option<PeerId>,
) -> Result<rustls::ClientConfig, certificate::GenError> {
    let (certificate, private_key) = certificate::generate(keypair)?;

    let mut crypto = rustls::ClientConfig::builder()
        .with_cipher_suites(verifier::CIPHERSUITES)
        .with_safe_default_kx_groups()
        .with_protocol_versions(verifier::PROTOCOL_VERSIONS)
        .expect("Cipher suites and kx groups are configured; qed")
        .with_custom_certificate_verifier(Arc::new(
            verifier::Libp2pCertificateVerifier::with_remote_peer_id(remote_peer_id),
        ))
        .with_single_cert(vec![certificate], private_key)
        .expect("Client cert key DER is valid; qed");
    crypto.alpn_protocols = vec![P2P_ALPN.to_vec()];

    Ok(crypto)
}
