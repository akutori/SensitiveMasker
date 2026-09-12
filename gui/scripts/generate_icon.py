"""アプリアイコンのマスター画像(1024x1024、透過)をPillowで生成する。

出力後、`bunx tauri icon <output>`でsrc-tauri/icons/配下の各プラットフォーム
向けファイル(ico/icns/各サイズPNG)を再生成する。

実行例: uv run --with pillow python gui/scripts/generate_icon.py
"""

from pathlib import Path

from PIL import Image, ImageDraw

SIZE = 1024
BG = (0x1C, 0x1F, 0x26, 255)
FG = (0xEC, 0xED, 0xEE, 255)
OUTPUT_PATH = Path(__file__).parent.parent / "src-tauri" / "app-icon-master.png"

# モックアップ(160x160、円半径48・スリット80x16 rx8・20度回転)と同じ比率で
# 1024x1024にスケールする(倍率 6.4)。
SCALE = SIZE / 160


def rounded_square(size, radius, fill):
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    draw.rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=fill)
    return img


def centered_rounded_rect_layer(canvas_size, rect_w, rect_h, radius, fill):
    layer = Image.new("RGBA", (canvas_size, canvas_size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    cx = cy = canvas_size / 2
    x0, y0 = cx - rect_w / 2, cy - rect_h / 2
    x1, y1 = cx + rect_w / 2, cy + rect_h / 2
    draw.rounded_rectangle([x0, y0, x1, y1], radius=radius, fill=fill)
    return layer


def main():
    icon = rounded_square(SIZE, int(SIZE * 0.2), BG)

    circle_layer = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    d = ImageDraw.Draw(circle_layer)
    r = 48 * SCALE
    cx = cy = SIZE / 2
    d.ellipse([cx - r, cy - r, cx + r, cy + r], fill=FG)

    slit_layer = centered_rounded_rect_layer(
        SIZE,
        rect_w=80 * SCALE,
        rect_h=16 * SCALE,
        radius=int(8 * SCALE),
        fill=BG,
    )
    slit_layer = slit_layer.rotate(20, resample=Image.BICUBIC, center=(SIZE / 2, SIZE / 2))

    icon = Image.alpha_composite(icon, circle_layer)
    icon = Image.alpha_composite(icon, slit_layer)

    icon.save(OUTPUT_PATH)
    print(f"saved {OUTPUT_PATH} ({icon.size[0]}x{icon.size[1]})")


if __name__ == "__main__":
    main()
