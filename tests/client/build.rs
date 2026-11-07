fn main() {
    println!("cargo:rerun-if-changed=../../proto/game.proto");
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc");
    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config
        .compile_protos(&["../../proto/game.proto"], &["../../proto"])
        .expect("compile game.proto");
}
