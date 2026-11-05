fn main() {
    // kws 的 build.rs 設的 rpath 只套用在 kws 自己的目標，app 執行檔要另外設：
    // 開發時從 models/ 載入 sherpa-onnx；打包後從 Contents/Frameworks 載入。
    let lib = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../models/sherpa-onnx-lib/lib");
    // 只在 debug 版加本機路徑，正式版不夾帶建置機器的路徑
    if std::env::var("PROFILE").as_deref() == Ok("debug") && let Ok(lib) = lib.canonicalize() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
    }
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@executable_path/../Frameworks");
    // whisper.cpp 的 Metal 程式用到 @available（___isPlatformVersionAtLeast），
    // 在 clang 的 compiler-rt 裡；rustc 連結時不會自動帶上，release 版會連結失敗。
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let out = std::process::Command::new("xcrun").args(["clang", "--print-resource-dir"]).output();
        if let Ok(o) = out && o.status.success() {
            let dir = std::path::PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()).join("lib/darwin");
            println!("cargo:rustc-link-search=native={}", dir.display());
            println!("cargo:rustc-link-lib=static=clang_rt.osx");
        }
    }
    tauri_build::build()
}
