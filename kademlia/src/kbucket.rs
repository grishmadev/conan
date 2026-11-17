use crate::nodeid::{Node, NodeId};
use std::collections::VecDeque;

pub const MAX_NODE: usize = 2;

#[derive(Debug, Clone)]
pub struct KBucket {
    max_size: usize,
    nodes: VecDeque<Node>,
}

impl KBucket {
    #[must_use]
    pub fn new(max_size: usize) -> Self {
        Self {
            max_size,
            nodes: VecDeque::new(),
        }
    }

    pub fn insert(&mut self, node: Node) -> bool {
        if let Some(idx) = self.nodes.iter().position(|f| f == &node) {
            self.nodes.remove(idx);
            self.nodes.push_back(node);
            return true;
        }

        if self.nodes.len() < self.max_size {
            self.nodes.push_back(node);
            return true;
        }

        // TODO: Bucket full. ping head before deciding
        false
    }

    pub fn remove_via_address(&mut self, addr: &str) -> bool {
        if let Some(idx) = self.nodes.iter().position(|f| f.address() == addr) {
            return self.remove(idx);
        }
        false
    }

    pub fn remove_via_id(&mut self, node: &NodeId) -> bool {
        if let Some(idx) = self.nodes.iter().position(|f| f.node_id() == node) {
            return self.remove(idx);
        }
        false
    }

    pub(crate) fn remove(&mut self, idx: usize) -> bool {
        self.nodes.remove(idx);
        true
    }

    #[must_use]
    pub fn list(&self) -> Vec<Node> {
        self.nodes.iter().cloned().collect::<Vec<Node>>()
    }

    #[must_use]
    pub fn max_size(&self) -> usize {
        self.max_size
    }
}
