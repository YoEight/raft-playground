use std::{sync::mpsc, thread, time::Duration};
use tokio::runtime::Handle;

use grpc::{ApiImpl, RaftImpl};
use machine::{NodeClient, Persistent};
use raft_common::{
    server::{ApiServer, RaftServer},
    NodeId,
};
use seed::Seed;
use tokio::task::JoinHandle;
use tonic::transport::{self, Server};

pub mod entry;
pub mod grpc;
pub mod machine;
pub mod options;
pub mod seed;

pub struct Node {
    id: NodeId,
    _seeds: Vec<Seed>,
    handle: thread::JoinHandle<()>,
    client: NodeClient,
    pub join: Option<JoinHandle<Result<(), transport::Error>>>,
    runtime: Handle,
}

impl Node {
    pub fn new(runtime: Handle, opts: options::Options) -> eyre::Result<Self> {
        let persistent = Persistent::load();
        let (sender, mailbox) = mpsc::channel();
        let client = NodeClient::new(sender);

        let mut seeds = Vec::new();

        let id = NodeId {
            host: "127.0.0.1".to_string(),
            port: opts.port as u32,
        };

        for seed_port in opts.seeds {
            let node_id = NodeId {
                host: "127.0.0.1".to_string(),
                port: seed_port as u32,
            };

            seeds.push(Seed::new(node_id, client.clone(), runtime.clone()));
        }

        if (seeds.len() + 1) % 2 == 0 {
            eyre::bail!(
                "Cluster size is an even number which could cause issues for leader election"
            );
        }

        let handle = machine::start(persistent, id.clone(), seeds.clone(), mailbox);

        Ok(Self {
            id,
            _seeds: seeds,
            client,
            handle,
            join: None,
            runtime,
        })
    }

    pub fn start(&mut self) {
        let addr = format!("{}:{}", self.id.host, self.id.port)
            .parse()
            .unwrap();
        let client = self.client.clone();

        self.runtime.spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(30)).await;
                if !client.tick() {
                    break;
                }
            }
        });

        // println!("Listening on {}:{}", self.id.host, self.id.port);
        let client = self.client.clone();
        let join = self.runtime.spawn(async move {
            Server::builder()
                .add_service(RaftServer::new(RaftImpl::new(client.clone())))
                .add_service(ApiServer::new(ApiImpl::new(client)))
                .serve(addr)
                .await
        });

        self.join = Some(join);
    }

    pub fn wait_for_completion(self) {
        let _ = self.handle.join();
    }
}
