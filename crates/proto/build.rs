fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    std::env::set_var("PROTOC", protoc);

    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let proto = manifest.join("../../proto/homeai.proto");
    let include = proto.parent().expect("proto dir");

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&[&proto], &[include])?;

    println!("cargo:rerun-if-changed={}", proto.display());
    Ok(())
}
