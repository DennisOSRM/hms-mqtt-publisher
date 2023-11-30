use protobuf_codegen::Codegen;
use std::io::Write;

static MOD_RS: &[u8] = b"
/// Generated from protobuf.
pub mod RealData;
/// Generated from protobuf.
pub mod GetConfig;
";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_files = ["src/protos/RealData.proto"];

    for path in &proto_files {
        println!("cargo:rerun-if-changed={path}");
    }

    let out_dir = std::env::var("OUT_DIR")?;

    Codegen::new()
        .pure()
        .cargo_out_dir("hoymiles")
        .inputs(proto_files)
        .include("src/protos")
        .run_from_script();

    std::fs::File::create(out_dir + "/mod.rs")?.write_all(MOD_RS)?;

    Ok(())
}
