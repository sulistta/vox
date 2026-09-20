"""Repeated Vocaela grounding benchmark for a static image."""

from __future__ import annotations

import argparse
import statistics
from pathlib import Path

from vocaela import DEFAULT_ENDPOINT, ground


def main() -> None:
    parser = argparse.ArgumentParser(description="Benchmark Vocaela grounding")
    parser.add_argument("--image", required=True, type=Path)
    parser.add_argument("--target", required=True)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--vocaela-endpoint", default=DEFAULT_ENDPOINT, help="Defaults to $VOCAELA_URL/v1/chat/completions")
    args = parser.parse_args()
    if not args.image.is_file():
        parser.error(f"image not found: {args.image}")
    if args.runs < 1:
        parser.error("--runs must be at least 1")

    totals: list[float] = []
    for number in range(1, args.runs + 1):
        result = ground(args.image, args.target, args.vocaela_endpoint)
        totals.append(result.total_grounding_ms)
        print(
            f"run={number} image_read_ms={result.image_read_ms:.1f} base64_ms={result.base64_ms:.1f} "
            f"http_request_ms={result.http_request_ms:.1f} parse_ms={result.parse_ms:.1f} "
            f"total_grounding_ms={result.total_grounding_ms:.1f} "
            f"bbox_center_normalized=({result.x_normalized:.4f}, {result.y_normalized:.4f})"
        )
        print(f"bbox_response={result.raw_response!r}")

    print(
        f"summary runs={args.runs} average_ms={statistics.mean(totals):.1f} "
        f"median_ms={statistics.median(totals):.1f} min_ms={min(totals):.1f} max_ms={max(totals):.1f}"
    )


if __name__ == "__main__":
    main()
