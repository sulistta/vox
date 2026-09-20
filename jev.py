"""Small Jev adapter: it selects from actions; application code owns the loop."""

from __future__ import annotations

import time
from dataclasses import dataclass
from typing import Any

ACTION_CRITERIA = {
    "click_search": "Focus the YouTube search field.",
    "type_query": "Type the requested query into the focused field.",
    "press_enter": "Submit the search.",
    "done": "The task is complete.",
}


@dataclass
class Decision:
    action: str
    latency_ms: float
    raw: Any


class Jev:
    def choose(self, goal: str, history: list[dict[str, Any]], candidates: list[str]) -> Decision:
        # Keep CLI help and image/parser diagnostics usable before dependencies are installed.
        from typesafe_sdk import Choice, TypeSafeClient

        criteria = {name: ACTION_CRITERIA[name] for name in candidates}
        started = time.perf_counter()
        with TypeSafeClient() as client:
            response = client.system_one(
                state={"goal": goal, "history": history},
                questions={
                    "next_action": Choice(
                        instructions="Choose the next desktop action toward the goal.",
                        criteria=criteria,
                    )
                },
            )
        latency_ms = (time.perf_counter() - started) * 1000
        action = response.choices["next_action"].choice
        if action not in candidates:
            raise RuntimeError(f"Jev returned an action outside the candidates: {action!r}")
        return Decision(action=action, latency_ms=latency_ms, raw=response)
