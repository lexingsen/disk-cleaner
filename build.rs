fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/disk-cleaner.ico");
    embed_resource::compile("app.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("failed to embed app.rc");
}
