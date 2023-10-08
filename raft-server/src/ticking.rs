use crate::state::NodeState;
use std::time::Duration;
use tokio::task::JoinHandle;

pub fn spawn_ticking_process(state: NodeState) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(30)).await;
            state.on_ticking().await
        }
    })
}
