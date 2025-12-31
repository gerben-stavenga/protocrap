// build.rs

fn main() {
    #[cfg(feature = "prost-compare")]
    {
        let out_dir = std::env::var("OUT_DIR").unwrap();
        println!("cargo:rerun-if-changed=proto/test.proto");
        println!("cargo:warning=Generating prost version...");
        prost_build::Config::new()
            .out_dir(&out_dir)
            .compile_protos(
                &["../codegen-tests/proto/test.proto"],
                &["../codegen-tests/proto/"],
            )
            .expect("prost codegen failed");
    }
}
