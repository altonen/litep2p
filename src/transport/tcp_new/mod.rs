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
    config::{Role, TransportConfig},
    crypto::{
        ed25519::Keypair,
        noise::{self, NoiseConfiguration},
        PublicKey,
    },
    error::{AddressError, Error, SubstreamError},
    peer_id::PeerId,
    transport::{
        Connection, ConnectionNew, Direction, Transport, TransportEvent, TransportNew,
        TransportService,
    },
    types::{ProtocolId, ProtocolType, RequestId, SubstreamId},
    DEFAULT_CHANNEL_SIZE,
};

use futures::future::BoxFuture;
use multiaddr::{Multiaddr, Protocol};
use tokio::net::{TcpListener, TcpStream};

use std::net::{IpAddr, SocketAddr};

/// Logging target for the file.
const LOG_TARGET: &str = "tcp";

#[derive(Debug)]
pub struct TcpConnection {
    stream: TcpStream,
}

impl ConnectionNew for TcpConnection {
    fn open_substream() -> Result<(), ()> {
        todo!();
    }

    fn close_substream() -> Result<(), ()> {
        todo!();
    }
}

#[derive(Debug)]
pub struct TcpTransport {
    listener: TcpListener,
}

impl TcpTransport {
    /// Extract socket address and `PeerId`, if found, from `address`.
    fn get_socket_address(address: &Multiaddr) -> crate::Result<(SocketAddr, Option<PeerId>)> {
        tracing::trace!(target: LOG_TARGET, ?address, "parse multi address");

        let mut iter = address.iter();
        let socket_address = match iter.next() {
            Some(Protocol::Ip6(address)) => match iter.next() {
                Some(Protocol::Tcp(port)) => SocketAddr::new(IpAddr::V6(address), port),
                protocol => {
                    tracing::error!(
                        target: LOG_TARGET,
                        ?protocol,
                        "invalid transport protocol, expected `Tcp`",
                    );
                    return Err(Error::AddressError(AddressError::InvalidProtocol));
                }
            },
            Some(Protocol::Ip4(address)) => match iter.next() {
                Some(Protocol::Tcp(port)) => SocketAddr::new(IpAddr::V4(address), port),
                protocol => {
                    tracing::error!(
                        target: LOG_TARGET,
                        ?protocol,
                        "invalid transport protocol, expected `Tcp`",
                    );
                    return Err(Error::AddressError(AddressError::InvalidProtocol));
                }
            },
            protocol => {
                tracing::error!(target: LOG_TARGET, ?protocol, "invalid transport protocol");
                return Err(Error::AddressError(AddressError::InvalidProtocol));
            }
        };

        let maybe_peer = match iter.next() {
            Some(Protocol::P2p(multihash)) => Some(PeerId::from_multihash(multihash)?),
            None => None,
            protocol => {
                tracing::error!(
                    target: LOG_TARGET,
                    ?protocol,
                    "invalid protocol, expected `P2p` or `None`"
                );
                return Err(Error::AddressError(AddressError::InvalidProtocol));
            }
        };

        Ok((socket_address, maybe_peer))
    }
}

#[async_trait::async_trait]
impl TransportNew for TcpTransport {
    type Connection = TcpConnection;

    /// Create new [`TcpTransport`].
    async fn new(listen_address: Multiaddr) -> crate::Result<Self> {
        let (listen_address, _) = Self::get_socket_address(&listen_address)?;

        tracing::info!(target: LOG_TARGET, ?listen_address, "start `TcpTransport`");

        Ok(Self {
            listener: TcpListener::bind(listen_address).await?,
        })
    }

    /// Open connection to remote peer at `address`.
    fn open_connection(
        &mut self,
        address: Multiaddr,
    ) -> crate::Result<BoxFuture<'static, crate::Result<Self::Connection>>> {
        tracing::debug!(target: LOG_TARGET, ?address, "open connection");

        let (socket_address, peer) = Self::get_socket_address(&address)?;

        Ok(Box::pin(async move {
            Ok(Self::Connection {
                stream: TcpStream::connect(socket_address).await?,
            })
        }))
    }

    /// Poll next connection from `TcpListener`.
    async fn next_connection(&mut self) -> Option<Self::Connection> {
        self.listener
            .accept()
            .await
            .ok()
            .map(|(stream, _)| Self::Connection { stream })
    }
}
