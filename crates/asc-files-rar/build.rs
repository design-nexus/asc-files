// SPDX-License-Identifier: MIT
//! The member stream's format is the engine's (MIT), shared as source so no engine code is linked.
fn main() {
    let shared = "../asc-files-engine/src/adapters/local_operations/archive/rar_wire.rs";
    println!("cargo:rerun-if-changed={shared}");
    let wire = std::fs::read_to_string(shared)
        .expect("the shared wire format")
        .replace("//!", "//")
        .replace("#[cfg(test)]\nmod tests;", "");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("build directory"));
    std::fs::write(output.join("rar_wire.rs"), wire).expect("stage the shared wire format");
}
