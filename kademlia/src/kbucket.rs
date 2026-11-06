use crate::nodeid::NodeId;
use std::collections::VecDeque;

pub const MAX_NODE: usize = 2;

#[derive(Debug, Clone)]
pub struct KBucket {
    max_size: usize,
    nodes: VecDeque<NodeId>,
}

impl KBucket {
    #[must_use]
    pub fn new(max_size: usize) -> Self {
        Self {
            max_size,
            nodes: VecDeque::new(),
        }
    }

    pub fn insert(&mut self, node: NodeId) -> bool {
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

    pub fn remove(&mut self, node: &NodeId) -> bool {
        if let Some(idx) = self.nodes.iter().position(|f| f == node) {
            self.nodes.remove(idx);
            return true;
        }
        false
    }

    #[must_use]
    pub fn list(&self) -> Vec<NodeId> {
        self.nodes.iter().copied().collect::<Vec<NodeId>>()
    }

    #[must_use]
    pub fn max_size(&self) -> usize {
        self.max_size
    }
}
