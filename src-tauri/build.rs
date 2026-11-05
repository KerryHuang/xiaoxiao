fn main() {
    // kws 的 build.rs 設的 rpath 只套用在 kws 自己的目標，app 執行檔要另外設：
    // 開發時從 models/ 載入 sherpa-onnx；打包後從 Contents/Frameworks 載入。
    let lib = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../models/sherpa-onnx-lib/lib");
    // 只在 debug 版加本機路徑，正式版不夾帶建置機器的路徑
    if std::env::var("PROFILE").as_deref() == Ok("debug") && let Ok(lib) = lib.canonicalize() {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,{}", lib.display());
    }
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path/../Frameworks");
    tauri_build::build()
}
