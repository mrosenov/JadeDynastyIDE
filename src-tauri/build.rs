fn main() {
    // Layouts, enums and masks are embedded with include_dir!, which Cargo does not track:
    // rebuild when a file under formats/ is added, changed or removed.
    println!("cargo:rerun-if-changed=formats");
    tauri_build::build()
}
