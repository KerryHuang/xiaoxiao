// 連結 scripts/fetch-models.sh 下載的 sherpa-onnx 預編 shared library。
fn main() {
    let lib = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../models/sherpa-onnx-lib/lib")
        .canonicalize()
        .expect("找不到 models/sherpa-onnx-lib/lib，先跑 scripts/fetch-models.sh");
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=dylib=sherpa-onnx-c-api");
    // 開發時從 models/ 載入；app bundle 內從 Contents/Frameworks 載入。
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
    println!("cargo:rerun-if-changed=build.rs");
}
