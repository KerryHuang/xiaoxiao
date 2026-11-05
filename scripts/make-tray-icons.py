"""產生選單列 template 圖示（黑色＋透明，macOS 會依深淺色自動反白）。不需第三方套件。"""
import math
import struct
import zlib
from pathlib import Path

S = 64
OUT = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"


def png(path: Path, alpha) -> None:
    rows = b"".join(b"\x00" + b"".join(bytes((0, 0, 0, alpha(x, y))) for x in range(S)) for y in range(S))
    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", S, S, 8, 6, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(rows, 9)) + chunk(b"IEND", b"")
    path.write_bytes(data)


def cover(f):
    """4x4 超取樣抗鋸齒。"""
    def a(x, y):
        n = sum(f(x + (i + 0.5) / 4, y + (j + 0.5) / 4) for i in range(4) for j in range(4))
        return round(255 * n / 16)
    return a


c = S / 2
def ring(x, y):
    return 20 <= math.hypot(x - c, y - c) <= 26
def disc(x, y):
    return math.hypot(x - c, y - c) <= 26
def paused(x, y):
    return ring(x, y) or (22 <= x <= 28 or 36 <= x <= 42) and 20 <= y <= 44
def warning(x, y):
    # 三角形外框＋驚嘆號
    top, bot = 8, 56
    inside = lambda m: y >= top + m * 1.8 and y <= bot - m and abs(x - c) <= (y - top) * 0.62 - m
    edge = inside(0) and not inside(5)
    mark = (abs(x - c) <= 3 and 24 <= y <= 41) or math.hypot(x - c, y - 48) <= 3.2
    return edge or mark


for name, f in [("idle", ring), ("listening", disc), ("paused", paused), ("warning", warning)]:
    png(OUT / f"tray-{name}.png", cover(f))
    print("寫入", f"tray-{name}.png")
