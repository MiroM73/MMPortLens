//! Windows: embeds the program icon (drawn by `src/icon.rs`) and the version information into
//! the executable, so Explorer and the taskbar show the MM PortLens icon (#15).

#[path = "src/icon.rs"]
#[allow(dead_code)]
mod icon;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/icon.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("mmportlens.ico");
    std::fs::write(&out, icon::ico(&[16, 20, 24, 32, 40, 48, 64, 256])).unwrap();
    let mut res = winresource::WindowsResource::new();
    res.set_icon(out.to_str().unwrap());
    res.set("FileDescription", "MM PortLens network connection viewer");
    res.set("ProductName", "MM PortLens");
    res.compile()
        .expect("compiling the Windows resources (rc.exe from the Windows SDK)");
}
