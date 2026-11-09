fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=assets/icon.ico");
        println!("cargo:rerun-if-changed=assets/app.rc");
        let _ = embed_resource::compile("assets/app.rc", embed_resource::NONE);
    }
}
