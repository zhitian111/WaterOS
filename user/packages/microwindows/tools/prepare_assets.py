#!/usr/bin/env python3
"""只使用 Python 标准库生成 Nano-X 可直接读取的 PPM 桌面资产。"""

from __future__ import annotations

import argparse
import struct
import zlib
from pathlib import Path


def decode_rgb_png(path: Path) -> tuple[int, int, bytes]:
    """解码 imagegen 产出的 8-bit、非隔行 RGB PNG。"""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise RuntimeError(f"not a PNG image: {path}")
    offset = 8
    width = height = 0
    compressed = bytearray()
    while offset < len(data):
        length = struct.unpack(">I", data[offset:offset + 4])[0]
        kind = data[offset + 4:offset + 8]
        payload = data[offset + 8:offset + 8 + length]
        offset += 12 + length
        if kind == b"IHDR":
            width, height, depth, color, compression, filtering, interlace = struct.unpack(
                ">IIBBBBB", payload)
            if (depth, color, compression, filtering, interlace) != (8, 2, 0, 0, 0):
                raise RuntimeError("wallpaper PNG must be non-interlaced 8-bit RGB")
        elif kind == b"IDAT":
            compressed.extend(payload)
        elif kind == b"IEND":
            break
    raw = zlib.decompress(bytes(compressed))
    stride = width * 3
    rows: list[bytearray] = []
    cursor = 0
    previous = bytearray(stride)
    for _ in range(height):
        filter_type = raw[cursor]
        cursor += 1
        row = bytearray(raw[cursor:cursor + stride])
        cursor += stride
        for index in range(stride):
            left = row[index - 3] if index >= 3 else 0
            above = previous[index]
            upper_left = previous[index - 3] if index >= 3 else 0
            if filter_type == 1:
                row[index] = (row[index] + left) & 0xff
            elif filter_type == 2:
                row[index] = (row[index] + above) & 0xff
            elif filter_type == 3:
                row[index] = (row[index] + ((left + above) >> 1)) & 0xff
            elif filter_type == 4:
                estimate = left + above - upper_left
                pa, pb, pc = abs(estimate - left), abs(estimate - above), abs(estimate - upper_left)
                predictor = left if pa <= pb and pa <= pc else above if pb <= pc else upper_left
                row[index] = (row[index] + predictor) & 0xff
            elif filter_type != 0:
                raise RuntimeError(f"unsupported PNG filter {filter_type}")
        rows.append(row)
        previous = row
    return width, height, b"".join(rows)


def write_ppm(path: Path, width: int, height: int, pixels: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(f"P6\n{width} {height}\n255\n".encode("ascii") + pixels)
    path.chmod(0o644)


def make_wallpaper(source: Path, output: Path) -> None:
    source_width, source_height, source_pixels = decode_rgb_png(source)
    width, height = 1280, 800
    # 居中裁成 16:10，再使用最近邻采样。源图已经接近目标尺寸，缩放损失很小。
    crop_width = min(source_width, source_height * width // height)
    crop_height = min(source_height, source_width * height // width)
    x0 = (source_width - crop_width) // 2
    y0 = (source_height - crop_height) // 2
    output_pixels = bytearray(width * height * 3)
    for y in range(height):
        source_y = y0 + y * crop_height // height
        for x in range(width):
            source_x = x0 + x * crop_width // width
            source_index = (source_y * source_width + source_x) * 3
            target_index = (y * width + x) * 3
            output_pixels[target_index:target_index + 3] = source_pixels[source_index:source_index + 3]
    write_ppm(output, width, height, bytes(output_pixels))


class Icon:
    """4 倍超采样的小图标画布，落盘时平均采样为 40×40。"""

    SCALE = 4
    SIZE = 40
    BACKGROUND = (10, 28, 45)
    CYAN = (34, 211, 238)
    PALE = (224, 247, 250)
    MUTED = (92, 148, 170)

    def __init__(self) -> None:
        self.width = self.SIZE * self.SCALE
        self.pixels = [self.BACKGROUND] * (self.width * self.width)

    def point(self, x: int, y: int, color: tuple[int, int, int]) -> None:
        if 0 <= x < self.width and 0 <= y < self.width:
            self.pixels[y * self.width + x] = color

    def rect(self, x0: int, y0: int, x1: int, y1: int,
             color: tuple[int, int, int]) -> None:
        scale = self.SCALE
        for y in range(y0 * scale, y1 * scale):
            for x in range(x0 * scale, x1 * scale):
                self.point(x, y, color)

    def circle(self, cx: int, cy: int, radius: int,
               color: tuple[int, int, int]) -> None:
        scale = self.SCALE
        cx, cy, radius = cx * scale, cy * scale, radius * scale
        for y in range(cy - radius, cy + radius + 1):
            for x in range(cx - radius, cx + radius + 1):
                if (x - cx) ** 2 + (y - cy) ** 2 <= radius ** 2:
                    self.point(x, y, color)

    def line(self, x0: int, y0: int, x1: int, y1: int, thickness: int,
             color: tuple[int, int, int]) -> None:
        scale = self.SCALE
        x0, y0, x1, y1 = x0 * scale, y0 * scale, x1 * scale, y1 * scale
        radius = max(1, thickness * scale // 2)
        steps = max(abs(x1 - x0), abs(y1 - y0), 1)
        for step in range(steps + 1):
            x = x0 + (x1 - x0) * step // steps
            y = y0 + (y1 - y0) * step // steps
            for py in range(y - radius, y + radius + 1):
                for px in range(x - radius, x + radius + 1):
                    if (px - x) ** 2 + (py - y) ** 2 <= radius ** 2:
                        self.point(px, py, color)

    def save(self, path: Path) -> None:
        scale = self.SCALE
        output = bytearray()
        for y in range(self.SIZE):
            for x in range(self.SIZE):
                samples = [self.pixels[(y * scale + sy) * self.width + x * scale + sx]
                           for sy in range(scale) for sx in range(scale)]
                output.extend(sum(pixel[channel] for pixel in samples) // len(samples)
                              for channel in range(3))
        write_ppm(path, self.SIZE, self.SIZE, bytes(output))


def icon_terminal(icon: Icon) -> None:
    icon.rect(5, 7, 35, 33, icon.CYAN)
    icon.rect(7, 9, 33, 31, icon.BACKGROUND)
    icon.line(11, 15, 16, 20, 2, icon.PALE)
    icon.line(16, 20, 11, 25, 2, icon.PALE)
    icon.line(20, 25, 28, 25, 2, icon.CYAN)


def icon_files(icon: Icon) -> None:
    icon.rect(5, 12, 35, 32, icon.CYAN)
    icon.rect(8, 8, 20, 15, icon.CYAN)
    icon.rect(7, 17, 33, 30, (18, 115, 145))
    icon.line(9, 20, 30, 20, 1, icon.PALE)


def icon_editor(icon: Icon) -> None:
    icon.rect(8, 5, 29, 35, icon.PALE)
    for y in (11, 16, 21, 26):
        icon.line(12, y, 25, y, 1, icon.MUTED)
    icon.line(18, 31, 34, 15, 4, icon.CYAN)
    icon.line(17, 32, 21, 31, 2, icon.PALE)


def icon_calculator(icon: Icon) -> None:
    icon.rect(8, 5, 32, 35, icon.CYAN)
    icon.rect(11, 8, 29, 15, icon.BACKGROUND)
    for y in (20, 27):
        for x in (13, 20, 27):
            icon.circle(x, y, 2, icon.PALE if x != 27 else icon.BACKGROUND)


def icon_game(icon: Icon) -> None:
    icon.circle(13, 23, 8, icon.CYAN)
    icon.circle(27, 23, 8, icon.CYAN)
    icon.rect(13, 15, 27, 30, icon.CYAN)
    icon.line(10, 23, 16, 23, 2, icon.BACKGROUND)
    icon.line(13, 20, 13, 26, 2, icon.BACKGROUND)
    icon.circle(26, 21, 2, icon.PALE)
    icon.circle(30, 25, 2, icon.PALE)


def icon_handheld(icon: Icon) -> None:
    icon.rect(8, 4, 32, 36, icon.CYAN)
    icon.rect(11, 7, 29, 22, icon.BACKGROUND)
    icon.rect(13, 9, 27, 20, (20, 92, 120))
    icon.line(12, 28, 18, 28, 2, icon.BACKGROUND)
    icon.line(15, 25, 15, 31, 2, icon.BACKGROUND)
    icon.circle(25, 27, 2, icon.PALE)
    icon.circle(28, 30, 2, icon.PALE)


def icon_clock(icon: Icon) -> None:
    icon.circle(20, 20, 15, icon.CYAN)
    icon.circle(20, 20, 12, icon.BACKGROUND)
    icon.line(20, 20, 20, 11, 2, icon.PALE)
    icon.line(20, 20, 27, 24, 2, icon.CYAN)
    icon.circle(20, 20, 2, icon.PALE)


def icon_eyes(icon: Icon) -> None:
    for cx in (13, 27):
        icon.circle(cx, 20, 9, icon.PALE)
        icon.circle(cx, 20, 4, icon.CYAN)
        icon.circle(cx, 20, 2, icon.BACKGROUND)


def icon_events(icon: Icon) -> None:
    icon.line(4, 22, 11, 22, 2, icon.MUTED)
    icon.line(11, 22, 15, 12, 2, icon.CYAN)
    icon.line(15, 12, 21, 30, 2, icon.CYAN)
    icon.line(21, 30, 26, 17, 2, icon.CYAN)
    icon.line(26, 17, 30, 22, 2, icon.CYAN)
    icon.line(30, 22, 36, 22, 2, icon.MUTED)


def make_icons(directory: Path) -> None:
    painters = {
        "terminal": icon_terminal,
        "files": icon_files,
        "editor": icon_editor,
        "calculator": icon_calculator,
        "doom": icon_game,
        "mgba": icon_handheld,
        "clock": icon_clock,
        "eyes": icon_eyes,
        "events": icon_events,
    }
    for name, painter in painters.items():
        icon = Icon()
        painter(icon)
        icon.save(directory / f"{name}.ppm")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    make_wallpaper(args.source, args.output / "wallpapers/wateros-waves.ppm")
    make_icons(args.output / "icons")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
