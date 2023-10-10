use crate::state::NodeState;
use tokio::sync::mpsc::{Sender, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

pub enum VoteMsg {
    VoteReceived { term: u64, granted: bool },
}

pub fn spawn_vote_listener(
    state: NodeState,
    mut recv: UnboundedReceiver<VoteMsg>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(msg) = recv.recv().await {
            match msg {
                VoteMsg::VoteReceived { term, granted } => {
                    state.on_vote_received(term, granted).await;
                }
            }
        }
    })
}
