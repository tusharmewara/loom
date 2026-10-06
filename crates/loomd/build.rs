fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile(&["proto/loom.proto"], &["proto"])
        .unwrap();
    
    // Copy generated file to a known location for inclusion
    let generated = std::path::Path::new(&out_dir).join("loom.rs");
    std::fs::copy(&generated, std::path::Path::new("src/grpc/loom.rs"))?;
    Ok(())
}