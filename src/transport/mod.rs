// Copyright 2023 litep2p developers
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

use crate::{
    config::TransportConfig,
    crypto::{ed25519::Keypair, noise::NoiseConfiguration},
    error::Error,
    peer_id::PeerId,
    types::{ProtocolId, ProtocolType, RequestId, SubstreamId},
};

use futures::{
    io::{AsyncRead, AsyncWrite},
    Stream,
};
use multiaddr::Multiaddr;
use tokio::sync::mpsc::Sender;

use std::fmt::Debug;

pub mod tcp;

// TODO: protocols for substream events
/// Supported transport types.
pub enum TransportType {
    /// TCP.
    Tcp(Multiaddr),
}

// TODO: can these be removed all together?
// TODO: these have to be moved elsewhere
pub trait Connection: AsyncRead + AsyncWrite + Unpin + Send + Debug + 'static {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send + Debug + 'static> Connection for T {}

/// Events emitted by the underlying transport.
#[derive(Debug)]
pub enum TransportEvent {
    SubstreamOpened(String, PeerId, Box<dyn Connection>),
    SubstreamClosed(String, PeerId),
    ConnectionEstablished(PeerId),
    ConnectionClosed(PeerId),
    DialFailure(Multiaddr),
}

#[async_trait::async_trait]
pub trait TransportService {
    /// Open connection to remote peer.
    ///
    /// Negotiate `noise`, perform the Noise handshake, negotiate `yamux` and return TODO
    async fn open_connection(&mut self, address: Multiaddr);

    /// Close connection to remote peer.
    async fn close_connection(&mut self, peer: PeerId);
}

#[async_trait::async_trait]
pub trait Transport {
    /// Start the underlying transport listener and return a handle which allows `litep2p` to
    // interact with the transport.
    async fn start(
        keypair: &Keypair,
        config: TransportConfig,
        tx: Sender<TransportEvent>,
    ) -> crate::Result<Box<dyn TransportService>>;
}
