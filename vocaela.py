"""OpenAI-multimodal client and tolerant coordinate parser for Vocaela."""

from __future__ import annotations

import base64
import json
import mimetypes
import os
import re
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

from PIL import Image


DEFAULT_VOCAELA_URL = os.environ.get("VOCAELA_URL", "http://127.0.0.1:8080").rstrip("/")
DEFAULT_ENDPOINT = f"{DEFAULT_VOCAELA_URL}/v1/chat/completions"


@dataclass
class Grounding:
    x_normalized: float
    y_normalized: float
    raw_response: str
    image_read_ms: float
    base64_ms: float
    http_request_ms: float
    parse_ms: float
    total_grounding_ms: float


def _number(value: Any) -> float | None:
    try:
        number = float(value)
    except (TypeError, ValueError):
        return None
    return number if 0.0 <= number <= 1.0 else None


def _from_mapping(value: Any) -> tuple[float, float] | None:
    """Find common coordinate JSON shapes, including action/coordinate wrappers."""
    if isinstance(value, dict):
        x = _number(value.get("center_x", value.get("x_normalized", value.get("x"))))
        y = _number(value.get("center_y", value.get("y_normalized", value.get("y"))))
        if x is not None and y is not None:
            return x, y
        for key in ("coordinates", "coordinate", "point", "center", "bbox", "bounding_box", "action"):
            found = _from_mapping(value.get(key))
            if found is not None:
                return found
        for nested in value.values():
            found = _from_mapping(nested)
            if found is not None:
                return found
    elif isinstance(value, list):
        if len(value) >= 2:
            x, y = _number(value[0]), _number(value[1])
            if x is not None and y is not None:
                return x, y
        for nested in value:
            found = _from_mapping(nested)
            if found is not None:
                return found
    return None


def parse_normalized_point(response_text: str) -> tuple[float, float]:
    """Accept Vocaela's bbox text and practical JSON/action coordinate variants."""
    bbox = re.search(r"\[\s*([-+]?\d*\.?\d+)\s*,\s*([-+]?\d*\.?\d+)\s*,\s*[-+]?\d*\.?\d+\s*,\s*[-+]?\d*\.?\d+\s*\]", response_text)
    if bbox:
        x, y = _number(bbox.group(1)), _number(bbox.group(2))
        if x is not None and y is not None:
            return x, y

    candidates = [response_text]
    candidates.extend(re.findall(r"```(?:json)?\s*(.*?)```", response_text, flags=re.DOTALL | re.IGNORECASE))
    for text in candidates:
        try:
            found = _from_mapping(json.loads(text.strip()))
        except json.JSONDecodeError:
            continue
        if found is not None:
            return found
    raise ValueError("Could not find normalized coordinates in Vocaela response")


def normalized_to_pixels(x: float, y: float, width: int, height: int) -> tuple[int, int]:
    return round(x * (width - 1)), round(y * (height - 1))


def _ground(encoded: str, mime: str, target: str, endpoint: str, image_read_ms: float, base64_ms: float, total_started: float) -> Grounding:
    """Send an already base64-encoded image to Vocaela."""
    body = {
        "model": "vocaela",
        "messages": [{"role": "user", "content": [
            {"type": "text", "text": target},
            {"type": "image_url", "image_url": {"url": f"data:{mime};base64,{encoded}"}},
        ]}],
        "temperature": 0,
        "max_tokens": 256,
    }
    request = Request(
        endpoint,
        data=json.dumps(body).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    started = time.perf_counter()
    try:
        with urlopen(request, timeout=60) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except HTTPError as error:
        raise RuntimeError(f"Vocaela returned HTTP {error.code}: {error.read().decode('utf-8', 'replace')}") from error
    except URLError as error:
        raise RuntimeError(f"Cannot reach Vocaela at {endpoint}: {error.reason}") from error
    http_request_ms = (time.perf_counter() - started) * 1000
    started = time.perf_counter()
    try:
        text = payload["choices"][0]["message"]["content"]
    except (KeyError, IndexError, TypeError) as error:
        raise RuntimeError(f"Unexpected Vocaela response: {payload!r}") from error
    x, y = parse_normalized_point(text)
    parse_ms = (time.perf_counter() - started) * 1000
    total_grounding_ms = (time.perf_counter() - total_started) * 1000
    return Grounding(x, y, text, image_read_ms, base64_ms, http_request_ms, parse_ms, total_grounding_ms)


def ground(image_path: str | Path, target: str, endpoint: str = DEFAULT_ENDPOINT) -> Grounding:
    """Ask Vocaela to locate a target in a static image path."""
    total_started = time.perf_counter()
    path = Path(image_path)
    mime = mimetypes.guess_type(path.name)[0] or "image/png"
    started = time.perf_counter()
    image_bytes = path.read_bytes()
    image_read_ms = (time.perf_counter() - started) * 1000
    started = time.perf_counter()
    encoded = base64.b64encode(image_bytes).decode("ascii")
    base64_ms = (time.perf_counter() - started) * 1000
    return _ground(encoded, mime, target, endpoint, image_read_ms, base64_ms, total_started)


def ground_image(image: Image.Image, target: str, endpoint: str = DEFAULT_ENDPOINT) -> Grounding:
    """Ask Vocaela using an in-memory live PipeWire frame; no screenshot file is written."""
    from io import BytesIO

    total_started = time.perf_counter()
    started = time.perf_counter()
    buffer = BytesIO()
    image.convert("RGB").save(buffer, format="PNG")
    image_bytes = buffer.getvalue()
    image_read_ms = (time.perf_counter() - started) * 1000
    started = time.perf_counter()
    encoded = base64.b64encode(image_bytes).decode("ascii")
    base64_ms = (time.perf_counter() - started) * 1000
    return _ground(encoded, "image/png", target, endpoint, image_read_ms, base64_ms, total_started)
