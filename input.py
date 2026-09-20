"""Minimal Wayland input adapter with explicit ydotool diagnostics."""

from __future__ import annotations

import json
import shutil
import subprocess
from dataclasses import asdict, dataclass


@dataclass
class CommandResult:
    command: list[str]
    stdout: str
    stderr: str
    returncode: int | None
    executed: bool

    def to_dict(self) -> dict[str, object]:
        return asdict(self)


class InputExecutionError(RuntimeError):
    def __init__(self, message: str, results: list[CommandResult]) -> None:
        super().__init__(message)
        self.results = results


class DesktopInput:
    def __init__(self, execute: bool = False) -> None:
        self.execute = execute
        self._mousemove_template: list[str] | None = None
        if self.execute:
            self._prepare_ydotool()

    @staticmethod
    def _log(result: CommandResult) -> None:
        print("input.command=", " ".join(result.command))
        print("input.result=", json.dumps(result.to_dict(), ensure_ascii=False))

    def _run(self, args: list[str]) -> CommandResult:
        if not self.execute:
            result = CommandResult(args, "", "", None, False)
            self._log(result)
            return result
        completed = subprocess.run(args, text=True, capture_output=True, check=False)
        result = CommandResult(args, completed.stdout, completed.stderr, completed.returncode, True)
        self._log(result)
        return result

    @staticmethod
    def _ydotoold_running() -> bool:
        return subprocess.run(["pgrep", "-x", "ydotoold"], capture_output=True, check=False).returncode == 0

    def _prepare_ydotool(self) -> None:
        if shutil.which("ydotool") is None:
            raise RuntimeError("--execute needs ydotool installed")
        if not self._ydotoold_running():
            print("WARNING: ydotoold is not running/available. ydotool may fail or report backend-unavailable warnings.")

        help_result = self._run(["ydotool", "mousemove", "--help"])
        if help_result.returncode != 0:
            raise InputExecutionError("Could not inspect ydotool mousemove syntax", [help_result])
        help_text = f"{help_result.stdout}\n{help_result.stderr}"
        if "--absolute" in help_text and "-x" in help_text and "-y" in help_text:
            self._mousemove_template = ["ydotool", "mousemove", "--absolute", "-x", "{x}", "-y", "{y}"]
        elif "--absolute" in help_text:
            self._mousemove_template = ["ydotool", "mousemove", "--absolute", "{x}", "{y}"]
        else:
            self._mousemove_template = ["ydotool", "mousemove", "{x}", "{y}"]
            print("WARNING: this ydotool does not advertise absolute mouse coordinates; using its positional syntax.")

    @staticmethod
    def _raise_on_failure(action: str, results: list[CommandResult]) -> None:
        if any(result.returncode not in (0, None) for result in results):
            raise InputExecutionError(f"ydotool {action} failed", results)

    def click(self, x: int, y: int) -> list[CommandResult]:
        if self.execute:
            assert self._mousemove_template is not None
            move = [part.format(x=x, y=y) for part in self._mousemove_template]
        else:
            move = ["ydotool", "mousemove", "--absolute", "-x", str(x), "-y", str(y)]
        results = [self._run(move)]
        self._raise_on_failure("mousemove", results)
        results.append(self._run(["ydotool", "click", "0xC0"]))
        self._raise_on_failure("click", results)
        return results

    def type_text(self, text: str) -> list[CommandResult]:
        results = [self._run(["ydotool", "type", "--key-delay", "20", text])]
        self._raise_on_failure("type", results)
        return results

    def press_enter(self) -> list[CommandResult]:
        results = [self._run(["ydotool", "key", "28:1", "28:0"])]
        self._raise_on_failure("key", results)
        return results
