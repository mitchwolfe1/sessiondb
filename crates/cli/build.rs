fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!(
        "cargo:rustc-env=SESSIONDB_BUILD_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
}
