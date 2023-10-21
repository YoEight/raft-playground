fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .type_attribute("NodeId", "#[derive(Eq, Ord, PartialOrd, Hash)]")
        .bytes(&["Entry.payload"])
        .compile(&["protos/raft.proto"], &["../protos/"])?;

    Ok(())
}
