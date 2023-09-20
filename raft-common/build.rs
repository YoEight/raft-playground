fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .bytes(&["EntriesReq.entries"])
        .compile(&["protos/raft.proto"], &["../protos/"])?;

    Ok(())
}
