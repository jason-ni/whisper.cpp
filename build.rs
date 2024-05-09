#[allow(unused_must_use)]
fn main() {

    cxx_build::bridge("rust/src/lib.rs")
        .includes([".",
            "rust/whisper_wrapper",
            "examples",
        ])
        .file("rust/whisper_wrapper/whisper_wrapper.cpp")
        .compile("whispercpp");
    println!("cargo:rerun-if-changed=rust/src/lib.rs");
}
