// 連結 scripts/fetch-models.sh 下載的 sherpa-onnx 預編 shared library。
fn main() {
    let lib = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../models/sherpa-onnx-lib/lib")
        .canonicalize()
        .expect("找不到 models/sherpa-onnx-lib/lib，先跑 scripts/fetch-models.sh");
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=dylib=sherpa-onnx-c-api");
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        copy_dlls_next_to_exe(&lib);
    } else {
        // 開發時從 models/ 載入；app bundle 內從 Contents/Frameworks 載入。
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    }
    println!("cargo:rerun-if-changed=build.rs");
}

/// Windows 沒有 rpath。System32 可能有另一版 onnxruntime.dll，搜尋順序排在 PATH 前面，
/// 所以 DLL 一定要放在 exe 同資料夾：target/<profile>/（app）、deps/（測試）、examples/。
fn copy_dlls_next_to_exe(lib: &std::path::Path) {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let profile = out.ancestors().nth(3).expect("OUT_DIR 結構不符預期");
    for dir in [profile.to_path_buf(), profile.join("deps"), profile.join("examples")] {
        std::fs::create_dir_all(&dir).unwrap();
        for f in std::fs::read_dir(lib).unwrap().flatten() {
            if f.path().extension().is_some_and(|e| e == "dll") {
                std::fs::copy(f.path(), dir.join(f.file_name())).unwrap();
            }
        }
    }
}
