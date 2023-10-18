use crate::state::NodeState;
use tokio::sync::mpsc::{Sender, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

pub enum IncomingMsg {
    VoteReceived {
        port: u16,
        term: u64,
        granted: bool,
    },
    AppendEntriesResp {
        port: u16,
        resp: tonic::Result<AppendEntriesResp>,
    },
}

pub struct AppendEntriesResp {
    pub term: u64,
    pub success: bool,
}

pub fn spawn_incoming_msg_listener(
    state: NodeState,
    mut recv: UnboundedReceiver<IncomingMsg>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(msg) = recv.recv().await {
            match msg {
                IncomingMsg::VoteReceived {
                    port,
                    term,
                    granted,
                } => {
                    state.on_vote_received(port, term, granted).await;
                }

                IncomingMsg::AppendEntriesResp { port, resp } => {
                    state.on_append_entries_resp(port, resp).await;
                }
            }
        }
    })
}
