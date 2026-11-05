#!/usr/bin/env bash
# 下載模型與 sherpa-onnx 函式庫到 models/，並對照 models/manifest.json 的 SHA256。
# 用法：scripts/fetch-models.sh            下載缺的檔案並驗證
#       scripts/fetch-models.sh --pin      第一次使用：把算出的 SHA256 寫回 manifest
#       scripts/fetch-models.sh --only <名稱>  只抓清單裡的這一項（CI 打包只需要該平台的 sherpa-onnx-lib）
set -euo pipefail
cd "$(dirname "$0")/.."
# python3 不一定存在於 Windows；PYTHONUTF8：runner 預設編碼印不出中文
PY=$(command -v python3 || command -v python)
PYTHONUTF8=1 "$PY" - "$@" <<'PY'
import hashlib, json, os, subprocess, sys, tarfile, urllib.request
args = sys.argv[1:]
pin = "--pin" in args
only = args[args.index("--only") + 1] if "--only" in args else None
mf_path = "models/manifest.json"
mf = json.load(open(mf_path, encoding="utf-8"))
if only and not any(f["name"] == only for f in mf["files"]):
    sys.exit(f"清單裡沒有 {only}")
os.makedirs("models/.downloads", exist_ok=True)
here = {"darwin": "macos", "win32": "windows"}.get(sys.platform, sys.platform)
for f in mf["files"]:
    if only and f["name"] != only:
        continue
    # 各平台的函式庫解到同一個資料夾，只抓這個平台的
    if not only and f.get("platform", here) != here:
        continue
    fname = f["url"].rsplit("/", 1)[-1]
    dl = os.path.join("models/.downloads", fname)
    if not os.path.exists(dl):
        print(f"下載 {f['name']} …", flush=True)
        part = dl + ".part"
        subprocess.run(["curl", "-fL", "--retry", "3", "-o", part, f["url"]], check=True)
        os.rename(part, dl)
    h = hashlib.sha256()
    with open(dl, "rb") as fp:
        for chunk in iter(lambda: fp.read(1 << 20), b""):
            h.update(chunk)
    digest = h.hexdigest()
    if f["sha256"] and f["sha256"] != digest:
        os.remove(dl)
        sys.exit(f"{f['name']} SHA256 不符（已刪除，請重跑）：{digest}")
    if not f["sha256"]:
        if not pin:
            sys.exit(f"{f['name']} 尚未釘 SHA256，請用 --pin 執行")
        f["sha256"] = digest
    if "extract_to" in f:
        dest = os.path.join("models", f["extract_to"])
        if not os.path.isdir(dest):
            os.makedirs(dest)
            with tarfile.open(dl) as t:
                for m in t.getmembers():
                    parts = m.name.split("/", 1)
                    if len(parts) < 2 or not parts[1] or m.issym() or m.islnk():
                        continue
                    if ".." in parts[1].split("/"):
                        continue
                    m.name = parts[1]
                    t.extract(m, dest)
    else:
        dest = os.path.join("models", f["save_as"])
        if not os.path.exists(dest):
            os.link(dl, dest)
    print(f"OK {f['name']}")
if pin:
    with open(mf_path, "w", encoding="utf-8") as fp:
        json.dump(mf, fp, ensure_ascii=False, indent=2)
        fp.write("\n")
PY
