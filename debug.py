"""Debug image output only; no OCR or capture functionality lives here."""

from __future__ import annotations

from pathlib import Path

from PIL import Image, ImageDraw


def draw_point(image_path: str | Path | Image.Image, x: int, y: int, output_path: str | Path) -> Path:
    output = Path(output_path)
    if isinstance(image_path, Image.Image):
        image = image_path.convert("RGB")
    else:
        with Image.open(image_path) as original:
            image = original.convert("RGB")
    radius = max(8, min(image.size) // 80)
    draw = ImageDraw.Draw(image)
    draw.ellipse((x - radius, y - radius, x + radius, y + radius), outline="red", width=4)
    draw.line((x - radius * 2, y, x + radius * 2, y), fill="red", width=2)
    draw.line((x, y - radius * 2, x, y + radius * 2), fill="red", width=2)
    image.save(output)
    return output
