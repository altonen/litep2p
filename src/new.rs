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
    codec::Codec,
    crypto::{ed25519::Keypair, PublicKey},
    error::Error,
    new_config::{Config, Litep2pConfig},
    peer_id::PeerId,
    protocol::{
        libp2p::new_ping::Ping,
        notification_new::{types::Config as NotificationConfig, NotificationProtocol},
        ConnectionEvent, ProtocolEvent, ProtocolSet,
    },
    transport::{
        tcp_new::TcpTransport, NewTransportEvent as TransportEvent, TransportError, TransportNew,
    },
    types::protocol::ProtocolName,
    DEFAULT_CHANNEL_SIZE, LOG_TARGET,
};

use futures::{stream::FuturesUnordered, Stream, StreamExt};
use multiaddr::{Multiaddr, Protocol};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::mpsc::{channel, Receiver, Sender},
};
use tokio_stream::{wrappers::ReceiverStream, StreamMap};

use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
};

/// Litep2p events.
#[derive(Debug)]
pub enum Litep2pEvent {
    /// Connection established to peer.
    ConnectionEstablished {
        /// Remote peer ID.
        peer: PeerId,

        /// Remote address.
        address: Multiaddr,
    },

    /// Failed to dial peer.
    DialFailure {
        /// Address of the peer.
        address: Multiaddr,

        /// Dial error.
        error: Error,
    },
}

/// [`Litep2p`] object.
pub struct Litep2p {
    /// Local peer ID.
    local_peer_id: PeerId,

    /// TCP transport.
    tcp: TcpTransport,

    /// Listen addresses.
    listen_addresses: Vec<Multiaddr>,

    /// Pending connections.
    pending_connections: HashMap<usize, Multiaddr>,
}

/// Transport context.
#[derive(Debug, Clone)]
pub struct TransportContext {
    /// Enabled protocols.
    pub protocols: HashMap<ProtocolName, Sender<ConnectionEvent>>,

    /// Keypair.
    pub keypair: Keypair,
}

pub struct ConnectionService {
    rx: Receiver<ConnectionEvent>,
    peers: HashMap<PeerId, Sender<ProtocolEvent>>,
}

impl ConnectionService {
    /// Create new [`ConnectionService`].
    pub fn new() -> (Self, Sender<ConnectionEvent>) {
        // TODO: maybe specify some other channel size
        let (tx, rx) = channel(DEFAULT_CHANNEL_SIZE);

        (
            Self {
                rx,
                peers: HashMap::new(),
            },
            tx,
        )
    }

    /// Get next event from the transport.
    pub async fn next_event(&mut self) -> Option<ConnectionEvent> {
        self.rx.recv().await
    }
}

impl TransportContext {
    /// Create new [`TransportContext`].
    pub fn new(keypair: Keypair) -> Self {
        Self {
            protocols: HashMap::new(),
            keypair,
        }
    }

    /// Add new protocol.
    pub fn add_protocol(&mut self, protocol: ProtocolName) -> crate::Result<ConnectionService> {
        let (service, tx) = ConnectionService::new();

        match self.protocols.insert(protocol.clone(), tx) {
            Some(_) => Err(Error::ProtocolAlreadyExists(protocol)),
            None => Ok(service),
        }
    }
}

impl Litep2p {
    /// Create new [`Litep2p`].
    pub async fn new(mut config: Litep2pConfig) -> crate::Result<Litep2p> {
        let local_peer_id = PeerId::from_public_key(&PublicKey::Ed25519(config.keypair.public()));
        let mut transport_ctx = TransportContext::new(config.keypair.clone());

        // start notification protocol event loops
        for (name, config) in config.notification_protocols.into_iter() {
            tracing::debug!(
                target: LOG_TARGET,
                protocol = ?name,
                "enable notification protocol",
            );

            let service = transport_ctx.add_protocol(name)?;
            tokio::spawn(async move { NotificationProtocol::new(service, config).run().await });
        }

        // start ping protocol event loop if enabled
        if let Some(config) = config.ping.take() {
            tracing::debug!(
                target: LOG_TARGET,
                protocol = ?config.protocol,
                "enable ping protocol",
            );

            let service = transport_ctx.add_protocol(config.protocol.clone())?;
            tokio::spawn(async move { Ping::new(service, config).run().await });
        }

        // TODO: go through all request-response protocols and start the protocol runners
        //       passing in the command the notification config

        // TODO: check if identify is enabled and if so, start identify event loop

        // enable tcp transport if the config exists
        let tcp = match config.tcp.take() {
            Some(config) => <TcpTransport as TransportNew>::new(transport_ctx, config).await?,
            None => panic!("tcp not enabled"),
        };
        let listen_addresses = vec![tcp.listen_address().clone()];

        Ok(Self {
            tcp,
            local_peer_id,
            listen_addresses,
            pending_connections: HashMap::new(),
        })
    }

    /// Get local peer ID.
    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    /// Get listen address for protocol.
    pub fn listen_addresses(&self) -> impl Iterator<Item = &Multiaddr> {
        self.listen_addresses.iter()
    }

    /// Attempt to connect to peer at `address`.
    ///
    /// If the transport specified by `address` is not supported, an error is returned.
    /// The connection is established in the background and its result is reported through
    /// [`Litep2p::next_event()`].
    pub fn connect(&mut self, address: Multiaddr) -> crate::Result<()> {
        let mut protocol_stack = address.protocol_stack();

        match protocol_stack.next() {
            Some("ip4") | Some("ip6") => {}
            transport => {
                tracing::error!(
                    target: LOG_TARGET,
                    ?transport,
                    "invalid transport, expected `ip4`/`ip6`"
                );
                return Err(Error::TransportNotSupported(address));
            }
        }

        match protocol_stack.next() {
            Some("tcp") => {
                let connection_id = self.tcp.open_connection(address.clone())?;
                self.pending_connections.insert(connection_id, address);
                Ok(())
            }
            protocol => {
                tracing::error!(
                    target: LOG_TARGET,
                    ?protocol,
                    "invalid protocol, expected `tcp`"
                );
                return Err(Error::TransportNotSupported(address));
            }
        }
    }

    /// Poll next event.
    pub async fn next_event(&mut self) -> crate::Result<Litep2pEvent> {
        loop {
            tokio::select! {
                event = self.tcp.next_event() => match event {
                    Ok(TransportEvent::ConnectionEstablished { peer, address }) => {
                        return Ok(Litep2pEvent::ConnectionEstablished { peer, address })
                    }
                    Ok(TransportEvent::DialFailure { error, address }) => {
                        return Ok(Litep2pEvent::DialFailure { address, error })
                    }
                    Err(error) => {
                        panic!("tcp transport failed: {error:?}");
                    }
                    event => {
                        tracing::info!(target: LOG_TARGET, ?event, "unhandle event from tcp");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        crypto::ed25519::Keypair,
        error::Error,
        new::{Litep2p, Litep2pEvent},
        new_config::{Litep2pConfig, Litep2pConfigBuilder},
        protocol::{
            libp2p::new_ping::{Config as PingConfig, PingEvent},
            notification_new::types::Config as NotificationConfig,
        },
        transport::tcp_new::config::TransportConfig as TcpTransportConfig,
        types::protocol::ProtocolName,
    };
    use futures::Stream;

    #[tokio::test]
    async fn initialize_litep2p() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();

        let (config1, _service1) = NotificationConfig::new(
            ProtocolName::from("/notificaton/1"),
            1337usize,
            vec![1, 2, 3, 4],
            Vec::new(),
        );
        let (config2, _service2) = NotificationConfig::new(
            ProtocolName::from("/notificaton/2"),
            1337usize,
            vec![1, 2, 3, 4],
            Vec::new(),
        );
        let (ping_config, ping_event_stream) = PingConfig::new(3);

        let mut config = Litep2pConfigBuilder::new()
            .with_tcp(TcpTransportConfig {
                listen_address: "/ip6/::1/tcp/0".parse().unwrap(),
            })
            .with_notification_protocol(config1)
            .with_notification_protocol(config2)
            .with_ping_protocol(ping_config)
            .build();

        let litep2p = Litep2p::new(config).await.unwrap();
    }

    // generate config for testing
    fn generate_config() -> (Litep2pConfig, Box<dyn Stream<Item = PingEvent> + Send>) {
        let keypair = Keypair::generate();
        let (ping_config, ping_event_stream) = PingConfig::new(3);

        (
            Litep2pConfigBuilder::new()
                .with_keypair(keypair)
                .with_tcp(TcpTransportConfig {
                    listen_address: "/ip6/::1/tcp/0".parse().unwrap(),
                })
                .with_ping_protocol(ping_config)
                .build(),
            ping_event_stream,
        )
    }

    #[tokio::test]
    async fn two_litep2ps_work() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();

        let (config1, ping_event_stream1) = generate_config();
        let (config2, ping_event_stream2) = generate_config();
        let mut litep2p1 = Litep2p::new(config1).await.unwrap();
        let mut litep2p2 = Litep2p::new(config2).await.unwrap();

        let address = litep2p2.listen_addresses().next().unwrap().clone();
        litep2p1.connect(address).unwrap();

        let (res1, res2) = tokio::join!(litep2p1.next_event(), litep2p2.next_event());

        assert!(std::matches!(
            res1,
            Ok(Litep2pEvent::ConnectionEstablished { .. })
        ));
        assert!(std::matches!(
            res2,
            Ok(Litep2pEvent::ConnectionEstablished { .. })
        ));
    }

    #[tokio::test]
    async fn dial_failure() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();

        let (config1, ping_event_stream1) = generate_config();
        let (config2, ping_event_stream2) = generate_config();
        let mut litep2p1 = Litep2p::new(config1).await.unwrap();
        let mut litep2p2 = Litep2p::new(config2).await.unwrap();

        litep2p1.connect("/ip6/::1/tcp/1".parse().unwrap()).unwrap();

        tokio::spawn(async move {
            loop {
                let _ = litep2p2.next_event().await;
            }
        });

        assert!(std::matches!(
            litep2p1.next_event().await,
            Ok(Litep2pEvent::DialFailure { .. })
        ));
    }
}
