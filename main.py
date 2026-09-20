"""Small Jev + Vocaela computer-use loop, offline or live through XDG Portal."""

from __future__ import annotations

import argparse
import json
import time
from abc import ABC, abstractmethod
from pathlib import Path
from typing import Any

from PIL import Image

from debug import draw_point
from input import DesktopInput, InputExecutionError
from jev import Jev
from vocaela import DEFAULT_ENDPOINT, ground_image, normalized_to_pixels
from wayland_portal import PortalInput, PortalSession


GOAL = "Search YouTube for rust programming"
QUERY = "rust programming"


class ScreenCapture(ABC):
    """Image source with a mapping from captured pixels to input coordinates."""

    @abstractmethod
    def capture_latest(self) -> Image.Image: ...

    @abstractmethod
    def input_coordinates(self, x_normalized: float, y_normalized: float, image: Image.Image) -> tuple[int, int]: ...


class StaticImageCapture(ScreenCapture):
    def __init__(self, image_path: Path) -> None:
        self.image_path = image_path

    def capture_latest(self) -> Image.Image:
        with Image.open(self.image_path) as image:
            return image.convert("RGB")

    def input_coordinates(self, x_normalized: float, y_normalized: float, image: Image.Image) -> tuple[int, int]:
        return normalized_to_pixels(x_normalized, y_normalized, *image.size)


class LiveScreenCapture(ScreenCapture):
    def __init__(self, portal: PortalSession) -> None:
        self.portal = portal

    def capture_latest(self) -> Image.Image:
        return self.portal.capture_latest()

    def input_coordinates(self, x_normalized: float, y_normalized: float, image: Image.Image) -> tuple[int, int]:
        # Vocaela coordinates refer to the captured frame. Portal absolute pointer
        # coordinates refer to logical_size when the backend provides it.
        return normalized_to_pixels(x_normalized, y_normalized, *self.portal.stream_logical_size)


def relevant_actions(history: list[dict[str, Any]]) -> list[str]:
    completed = [entry["action"] for entry in history if entry.get("status") == "completed"]
    if "click_search" not in completed:
        return ["click_search", "done"]
    if "type_query" not in completed:
        return ["type_query", "done"]
    if "press_enter" not in completed:
        return ["press_enter", "done"]
    return ["done"]


def _failed_event(event: dict[str, Any], error: InputExecutionError, history: list[dict[str, Any]]) -> None:
    event.update({"status": "failed", "error": str(error), "input": [item.to_dict() for item in error.results]})
    history.append(event)
    print("history=", json.dumps(history, ensure_ascii=False))


def run(goal: str, screen: ScreenCapture, desktop: Any, endpoint: str, debug_path: Path, settle_ms: int) -> None:
    if goal != GOAL:
        raise ValueError(f"This MVP supports only: {GOAL!r}")
    history: list[dict[str, Any]] = []
    chooser = Jev()

    for step in range(1, 11):
        step_started = time.perf_counter()
        candidates = relevant_actions(history)
        decision = chooser.choose(goal, history, candidates)
        print(f"\nstep={step} jev.decision={decision.action} jev_ms={decision.latency_ms:.1f}")
        if decision.action == "done":
            history.append({"action": "done", "status": "completed"})
            print(f"capture_ms=0.0 vocaela_ms=0.0 input_ms=0.0 total_step_ms={(time.perf_counter() - step_started) * 1000:.1f}")
            print("Task finished.")
            return

        event: dict[str, Any] = {"action": decision.action, "status": "pending", "jev_ms": decision.latency_ms}
        capture_ms = vocaela_ms = input_ms = 0.0
        if decision.action == "click_search":
            # Always acquire the latest frame immediately before grounding.
            started = time.perf_counter()
            image = screen.capture_latest()
            capture_ms = (time.perf_counter() - started) * 1000
            instruction = (
                "Click the search input inside the YouTube webpage header, at the top center. "
                "Ignore terminal windows, command text, tabs, and title bars."
            )
            print(f"capture_ms={capture_ms:.1f} vocaela.instruction={instruction!r}")
            result = ground_image(image, instruction, endpoint)
            vocaela_ms = result.total_grounding_ms
            debug_x, debug_y = normalized_to_pixels(result.x_normalized, result.y_normalized, *image.size)
            input_x, input_y = screen.input_coordinates(result.x_normalized, result.y_normalized, image)
            debug_file = draw_point(image, debug_x, debug_y, debug_path)
            print(f"vocaela.response={result.raw_response!r}")
            print(f"vocaela_ms={vocaela_ms:.1f} image_read_ms={result.image_read_ms:.1f} base64_ms={result.base64_ms:.1f} http_request_ms={result.http_request_ms:.1f} parse_ms={result.parse_ms:.1f}")
            print(f"coordinate.normalized=({result.x_normalized:.4f}, {result.y_normalized:.4f}) captured_pixels=({debug_x}, {debug_y}) input_coordinates=({input_x}, {input_y}) debug_image={debug_file}")
            try:
                started = time.perf_counter()
                input_results = desktop.click(input_x, input_y)
                input_ms = (time.perf_counter() - started) * 1000
            except InputExecutionError as error:
                _failed_event(event, error, history)
                raise
            event.update({"normalized": [result.x_normalized, result.y_normalized], "pixels": [debug_x, debug_y], "input_coordinates": [input_x, input_y]})
        elif decision.action == "type_query":
            try:
                started = time.perf_counter()
                input_results = desktop.type_text(QUERY)
                input_ms = (time.perf_counter() - started) * 1000
            except InputExecutionError as error:
                _failed_event(event, error, history)
                raise
            event["text"] = QUERY
        elif decision.action == "press_enter":
            try:
                started = time.perf_counter()
                input_results = desktop.press_enter()
                input_ms = (time.perf_counter() - started) * 1000
            except InputExecutionError as error:
                _failed_event(event, error, history)
                raise

        if settle_ms:
            time.sleep(settle_ms / 1000)
        total_step_ms = (time.perf_counter() - step_started) * 1000
        event.update({
            "status": "completed", "input": [item.to_dict() for item in input_results],
            "capture_ms": capture_ms, "vocaela_ms": vocaela_ms, "input_ms": input_ms, "total_step_ms": total_step_ms,
        })
        history.append(event)
        print(f"capture_ms={capture_ms:.1f} vocaela_ms={vocaela_ms:.1f} input_ms={input_ms:.1f} total_step_ms={total_step_ms:.1f}")
        print("history=", json.dumps(history, ensure_ascii=False))
    raise RuntimeError("Stopped after 10 steps")


def main() -> None:
    parser = argparse.ArgumentParser(description="Small Jev + Vocaela computer-use MVP")
    parser.add_argument("--goal", required=True)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--image", type=Path, help="Offline static screenshot")
    source.add_argument("--live", action="store_true", help="Use one persistent XDG Portal + PipeWire session")
    parser.add_argument("--execute", action="store_true", help="Inject input; dry-run is the default")
    parser.add_argument("--settle-ms", type=int, default=300, help="Wait after each input before the next capture (default: 300)")
    parser.add_argument("--vocaela-endpoint", default=DEFAULT_ENDPOINT, help="Defaults to $VOCAELA_URL/v1/chat/completions")
    parser.add_argument("--debug-image", type=Path, default=Path("debug_point.png"))
    args = parser.parse_args()
    if args.settle_ms < 0:
        parser.error("--settle-ms must not be negative")
    if args.image:
        if not args.image.is_file():
            parser.error(f"image not found: {args.image}")
        run(args.goal, StaticImageCapture(args.image), DesktopInput(args.execute), args.vocaela_endpoint, args.debug_image, args.settle_ms)
    else:
        with PortalSession() as portal:
            run(args.goal, LiveScreenCapture(portal), PortalInput(portal, args.execute), args.vocaela_endpoint, args.debug_image, args.settle_ms)


if __name__ == "__main__":
    main()
