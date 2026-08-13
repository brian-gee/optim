fn main() {
    println!("cargo:rerun-if-changed=assets/optim.ico");
    winres::WindowsResource::new()
        .set_icon_with_id("assets/optim.ico", "app_icon")
        .compile()
        .expect("failed to embed icon resource");
}
