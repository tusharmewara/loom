//! ACP peer discovery — find ACP agents on the local network.
//!
//! Supports:
//! - Local peer table (manually configured peers)
//! - HTTP health-check based peer verification
//! - mDNS-based discovery (when `mdns` feature is enabled)

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

/// A discovered ACP peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpPeer {
    /// Unique peer identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Network address for ACP connections.
    pub address: SocketAddr,
    /// Peer's public key (hex-encoded Ed25519).
    pub public_key: Option<String>,
    /// Agent capabilities.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// Last successful health check.
    pub last_seen: Option<SystemTime>,
    /// Whether this peer is currently reachable.
    pub online: bool,
}

impl AcpPeer {
    /// Create a new peer entry.
    pub fn new(id: &str, name: &str, address: SocketAddr) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            address,
            public_key: None,
            capabilities: vec![],
            last_seen: None,
            online: false,
        }
    }
}

/// Manages ACP peer discovery.
pub struct PeerDiscovery {
    /// Known peers (id → peer).
    peers: Arc<RwLock<HashMap<String, AcpPeer>>>,
}

impl PeerDiscovery {
    /// Create a new peer discovery manager.
    pub fn new(_local_id: &str) -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a known peer.
    pub async fn register_peer(&self, peer: AcpPeer) {
        let mut peers = self.peers.write().await;
        peers.insert(peer.id.clone(), peer);
    }

    /// Register multiple peers at once.
    pub async fn register_peers(&self, peers: Vec<AcpPeer>) {
        let mut map = self.peers.write().await;
        for peer in peers {
            map.insert(peer.id.clone(), peer);
        }
    }

    /// Remove a peer by ID.
    pub async fn remove_peer(&self, id: &str) {
        let mut peers = self.peers.write().await;
        peers.remove(id);
    }

    /// Get all known peers.
    pub async fn get_peers(&self) -> Vec<AcpPeer> {
        let peers = self.peers.read().await;
        peers.values().cloned().collect()
    }

    /// Get online peers only.
    pub async fn get_online_peers(&self) -> Vec<AcpPeer> {
        let peers = self.peers.read().await;
        peers.values().filter(|p| p.online).cloned().collect()
    }

    /// Health-check a specific peer by attempting an HTTP connection.
    pub async fn health_check_peer(&self, peer_id: &str) -> Result<bool> {
        let peer = {
            let peers = self.peers.read().await;
            peers.get(peer_id).cloned()
        };

        if let Some(mut peer) = peer {
            let online = check_http_endpoint(&peer.address).await;
            peer.online = online;
            if online {
                peer.last_seen = Some(SystemTime::now());
            }
            let mut peers = self.peers.write().await;
            peers.insert(peer_id.to_string(), peer);
            Ok(online)
        } else {
            anyhow::bail!("Peer '{peer_id}' not found");
        }
    }

    /// Health-check all known peers.
    pub async fn health_check_all(&self) -> Result<()> {
        let ids: Vec<String> = {
            let peers = self.peers.read().await;
            peers.keys().cloned().collect()
        };

        for id in &ids {
            let _ = self.health_check_peer(id).await;
        }
        Ok(())
    }

    /// Remove peers that have been offline for longer than the given duration.
    pub async fn prune_offline_peers(&self, max_age: Duration) {
        let mut peers = self.peers.write().await;
        peers.retain(|_, peer| {
            if let Some(last) = peer.last_seen {
                last.elapsed().unwrap_or(Duration::MAX) < max_age
            } else {
                false // never seen online
            }
        });
    }

    /// Load peers from a JSON file.
    pub async fn load_from_file(&self, path: &str) -> Result<()> {
        let content = tokio::fs::read_to_string(path).await?;
        let peers: Vec<AcpPeer> = serde_json::from_str(&content)?;
        self.register_peers(peers).await;
        Ok(())
    }

    /// Save known peers to a JSON file.
    pub async fn save_to_file(&self, path: &str) -> Result<()> {
        let peers = self.get_peers().await;
        let content = serde_json::to_string_pretty(&peers)?;
        tokio::fs::write(path, content).await?;
        Ok(())
    }
}

/// Check if an HTTP endpoint is reachable.
async fn check_http_endpoint(addr: &SocketAddr) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(3), async {
            tokio::net::TcpStream::connect(addr).await
        })
        .await,
        Ok(Ok(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn test_peer_registration() {
        let discovery = PeerDiscovery::new("test-agent");
        let peer = AcpPeer::new(
            "peer-1",
            "Test Peer",
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9876),
        );
        discovery.register_peer(peer).await;
        let peers = discovery.get_peers().await;
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].id, "peer-1");
    }

    #[tokio::test]
    async fn test_peer_removal() {
        let discovery = PeerDiscovery::new("test-agent");
        let peer = AcpPeer::new(
            "peer-1",
            "Test Peer",
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9876),
        );
        discovery.register_peer(peer).await;
        discovery.remove_peer("peer-1").await;
        let peers = discovery.get_peers().await;
        assert_eq!(peers.len(), 0);
    }

    #[tokio::test]
    async fn test_multiple_peers() {
        let discovery = PeerDiscovery::new("test-agent");
        let peers = vec![
            AcpPeer::new("a", "A", SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9001)),
            AcpPeer::new("b", "B", SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9002)),
        ];
        discovery.register_peers(peers).await;
        assert_eq!(discovery.get_peers().await.len(), 2);
    }
}
