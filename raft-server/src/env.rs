use tonic::transport::Channel;
use uuid::Uuid;

#[derive(Debug)]
pub struct Seed {
    pub id: Uuid,
    pub host: String,
    pub port: u16,
    pub channel: Option<Channel>,
}
