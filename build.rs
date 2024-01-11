extern crate bindgen;

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let splinterdb_path = PathBuf::from("/users/hao01/splinterdb-sys/splinterdb");
    env::set_var("CC", "clang");
    env::set_var("LD", "clang");

    Command::new("make")
        .current_dir(&splinterdb_path)
        .status()
        .expect("Failed to build splinterdb");

    let splinterdb_lib = splinterdb_path.join("build/release/lib");
    let splinterdb_include = splinterdb_path.join("include");

    println!(
        "cargo:rustc-link-search=native={}",
        splinterdb_lib.display()
    );
    println!(
        "cargo:rustc-link-arg=-Wl,-rpath,{}",
        splinterdb_lib.display()
    );
    println!("cargo:rustc-link-lib=dylib=splinterdb");

    // Tell cargo to invalidate the built crate whenever the wrapper changes
    println!("cargo:rerun-if-changed=wrapper.h");

    // The bindgen::Builder is the main entry point
    // to bindgen, and lets you build up options for
    // the resulting bindings.
    let bindings = bindgen::Builder::default()
        .no_copy("splinterdb.*")
        .no_copy("writable_buffer")
        .no_copy("data_config")
        .allowlist_type("splinterdb.*")
        .allowlist_function("splinterdb.*")
        .allowlist_function("default_data_config.*")
        .allowlist_function("merge.*")
        .allowlist_var("SPLINTERDB.*")
        .allowlist_var(".*_SIZE")
        .clang_arg("-DSPLINTERDB_PLATFORM_DIR=platform_linux")
        .header("wrapper.h")
        .clang_arg(format!("-I{}", splinterdb_include.display()))
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
