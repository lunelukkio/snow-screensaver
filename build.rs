//! Embeds the application icon (resource id 1) into the executable.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=installer/snow.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("installer/snow.ico");
        resource
            .compile()
            .expect("failed to embed the icon resource");
    }
}
