#!/usr/bin/env python3
"""由 src-tauri/icons/source.svg 生成 PureSlate 全套平台图标。

- 光栅化：本机 Edge headless 直接按目标尺寸渲染矢量（16px 也由矢量直出，
  避免大图缩放糊边；透明背景用 --default-background-color=00000000）。
- 产出：Windows ICO（16–256 多尺寸）、Tauri PNG、Store Logo 系列、
  macOS ICNS、网页 favicon。
- 无第三方依赖（Pillow 仅用于校验/兜底，不参与渲染）。

用法：python tools/gen-icons.py
"""
from __future__ import annotations

import os
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ICON_DIR = ROOT / "src-tauri" / "icons"
SOURCE = ICON_DIR / "source.svg"
PUBLIC_DIR = ROOT / "public"

EDGE_CANDIDATES = [
    os.environ.get("EDGE_PATH", ""),
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
]

ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
PNG_TARGETS = {
    "icon.png": 1024,
    "32x32.png": 32,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "Square30x30Logo.png": 30,
    "Square44x44Logo.png": 44,
    "Square71x71Logo.png": 71,
    "Square89x89Logo.png": 89,
    "Square107x107Logo.png": 107,
    "Square142x142Logo.png": 142,
    "Square150x150Logo.png": 150,
    "Square284x284Logo.png": 284,
    "Square310x310Logo.png": 310,
    "StoreLogo.png": 50,
}
ICNS_CHUNKS = [
    ("ic11", 32),
    ("ic12", 64),
    ("ic07", 128),
    ("ic08", 256),
    ("ic13", 256),
    ("ic09", 512),
    ("ic14", 512),
    ("ic10", 1024),
]


def find_edge() -> str:
    for cand in EDGE_CANDIDATES:
        if cand and Path(cand).is_file():
            return cand
    print("未找到 Edge，可通过环境变量 EDGE_PATH 指定 msedge.exe 路径", file=sys.stderr)
    raise SystemExit(1)


def render(edge: str, size: int, work: Path, profile: Path) -> bytes:
    svg = SOURCE.read_text(encoding="utf-8")
    svg = svg.replace(
        '<svg xmlns="http://www.w3.org/2000/svg"',
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}"',
        1,
    )
    html = work / f"wrap-{size}.html"
    html.write_text(
        "<!doctype html><html><head><meta charset='utf-8'>"
        "<style>html,body{margin:0;background:transparent;overflow:hidden}svg{display:block}"
        "</style></head><body>"
        + svg
        + "</body></html>",
        encoding="utf-8",
    )
    out = work / f"icon-{size}.png"
    subprocess.run(
        [
            edge,
            "--headless=new",
            "--disable-gpu",
            "--hide-scrollbars",
            "--force-device-scale-factor=1",
            "--default-background-color=00000000",
            f"--window-size={size},{size}",
            "--virtual-time-budget=2000",
            f"--user-data-dir={profile}",
            f"--screenshot={out}",
            html.as_uri(),
        ],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    data = out.read_bytes()
    check_png(data, size)
    return data


def check_png(data: bytes, size: int) -> None:
    """校验 PNG 头里的宽高，避免 Edge 视口尺寸偏差悄悄产出错图。"""
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit("渲染结果不是 PNG")
    w, h = struct.unpack(">II", data[16:24])
    if (w, h) != (size, size):
        raise SystemExit(f"渲染尺寸不符：期望 {size}，实际 {w}x{h}")


def build_ico(pngs: dict[int, bytes], path: Path) -> None:
    sizes = sorted(pngs)
    header = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    entries = []
    blobs = []
    for s in sizes:
        blob = pngs[s]
        dim = 0 if s >= 256 else s
        entries.append(struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset))
        offset += len(blob)
        blobs.append(blob)
    path.write_bytes(header + b"".join(entries) + b"".join(blobs))


def build_icns(pngs: dict[int, bytes], path: Path) -> None:
    chunks = b"".join(
        kind.encode("ascii") + struct.pack(">I", len(pngs[size]) + 8) + pngs[size]
        for kind, size in ICNS_CHUNKS
    )
    path.write_bytes(b"icns" + struct.pack(">I", len(chunks) + 8) + chunks)


def main() -> None:
    if not SOURCE.is_file():
        raise SystemExit(f"缺少矢量源：{SOURCE}")
    edge = find_edge()
    work = Path(tempfile.mkdtemp(prefix="pureslate-icons-"))
    profile = work / "profile"
    try:
        needed = sorted(set(ICO_SIZES + list(PNG_TARGETS.values()) + [s for _, s in ICNS_CHUNKS]))
        rendered = {size: render(edge, size, work, profile) for size in needed}

        for name, size in PNG_TARGETS.items():
            (ICON_DIR / name).write_bytes(rendered[size])
        build_ico({s: rendered[s] for s in ICO_SIZES}, ICON_DIR / "icon.ico")
        build_icns(rendered, ICON_DIR / "icon.icns")

        PUBLIC_DIR.mkdir(exist_ok=True)
        shutil.copyfile(SOURCE, PUBLIC_DIR / "favicon.svg")
        (PUBLIC_DIR / "favicon.png").write_bytes(rendered[32])

        print(f"生成完成：{len(PNG_TARGETS)} 个 PNG、ICO {ICO_SIZES}、ICNS、favicon")
    finally:
        shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
