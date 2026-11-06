use crate::{kbucket::KBucket, nodeid::NodeId};
pub mod kbucket;
pub mod nodeid;

const MAX_NODES: usize = 20;
const MAX_BUCKETS: usize = 256;

pub struct RoutingTable {
    local_node: NodeId,
    buckets: Vec<KBucket>,
}

impl RoutingTable {
    #[must_use]
    pub fn new(local_node: NodeId) -> Self {
        let mut buckets = Vec::with_capacity(MAX_BUCKETS);
        for _ in 0..256 {
            buckets.push(KBucket::new(MAX_NODES));
        }
        Self {
            local_node,
            buckets,
        }
    }

    pub fn add_peer(&mut self, node: NodeId) -> bool {
        let idx = self.local_node.bucket_index(&node);
        self.buckets[idx].insert(node)
    }

    pub fn remove_peer(&mut self, node: &NodeId) -> bool {
        let idx = self.local_node.bucket_index(node);
        self.buckets[idx].remove(node)
    }

    #[must_use]
    pub fn peers(&self) -> Vec<String> {
        let mut res = vec![];
        for bucket in &self.buckets {
            for node in bucket.list() {
                let address = node.to_string();
                res.push(address);
            }
        }
        res
    }
}
