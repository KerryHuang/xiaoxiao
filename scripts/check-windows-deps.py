#!/usr/bin/env python3
"""檢查安裝目錄：每個 exe／dll 用到的 VC++ runtime 都要在同一個資料夾。
CI 機器本身裝了 runtime，啟動測試抓不到缺漏，所以直接看匯入表。需要 pefile。"""
import re
import sys
from pathlib import Path

import pefile

VC_RUNTIME = re.compile(r"^(vcruntime|msvcp|concrt|vcomp|vccorlib|mfc)", re.I)

folder = Path(sys.argv[1])
present = {p.name.lower() for p in folder.iterdir()}
missing = {}
for f in folder.iterdir():
    if f.suffix.lower() not in (".exe", ".dll"):
        continue
    pe = pefile.PE(str(f), fast_load=True)
    pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_IMPORT"]])
    for entry in getattr(pe, "DIRECTORY_ENTRY_IMPORT", []):
        name = entry.dll.decode().lower()
        if VC_RUNTIME.match(name) and name not in present:
            missing.setdefault(name, []).append(f.name)
if missing:
    sys.exit(f"安裝目錄缺 VC++ runtime：{missing}")
print("VC++ runtime 齊全")
