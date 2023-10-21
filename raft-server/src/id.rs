use raft_common::NodeId;
use std::fmt::Debug;

pub trait NodeIdent: PartialEq + Eq + PartialOrd + Ord + Clone + Debug {}

impl NodeIdent for NodeId {}
