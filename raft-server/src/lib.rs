use std::{sync::mpsc, thread, time::Duration};

use grpc::{ApiImpl, RaftImpl};
use machine::{NodeClient, Persistent};
use raft_common::{
    server::{ApiServer, RaftServer},
    NodeId,
};
use seed::Seed;
use tokio::task;
use tonic::transport::{self, Server};

pub mod entry;
pub mod grpc;
pub mod machine;
pub mod options;
pub mod seed;

pub struct Node {
    id: NodeId,
    seeds: Vec<Seed>,
    handle: thread::JoinHandle<()>,
    client: NodeClient,
}

impl Node {
    pub fn new(opts: options::Options) -> eyre::Result<Self> {
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

            seeds.push(Seed::new(node_id, client.clone()));
        }

        if (seeds.len() + 1) % 2 == 0 {
            panic!("Cluster size is an even number which could cause issues for leader election");
        }

        let handle = machine::start(persistent, id.clone(), seeds.clone(), mailbox);

        Ok(Self {
            id,
            seeds,
            client,
            handle,
        })
    }

    pub fn start(&self) -> task::JoinHandle<Result<(), transport::Error>> {
        let addr = format!("{}:{}", self.id.host, self.id.port)
            .parse()
            .unwrap();
        let client = self.client.clone();
        let mut ticking = tokio::time::interval(Duration::from_millis(5));

        tokio::spawn(async move {
            loop {
                ticking.tick().await;
                if !client.tick() {
                    break;
                }
            }
        });

        println!("Listening on {}:{}", self.id.host, self.id.port);
        let client = self.client.clone();
        tokio::spawn(async move {
            Server::builder()
                .add_service(RaftServer::new(RaftImpl::new(client.clone())))
                .add_service(ApiServer::new(ApiImpl::new(client)))
                .serve(addr)
                .await
        })
    }

    pub fn wait_for_completion(self) {
        let _ = self.handle.join();
    }
}
