//! Integre l'icone et les metadonnees dans l'executable Windows.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/ClearMic.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/ClearMic.ico");
        res.set("ProductName", "ClearMic");
        res.set("FileDescription", "ClearMic");
        res.compile().expect("compilation des ressources Windows");
    }
}
