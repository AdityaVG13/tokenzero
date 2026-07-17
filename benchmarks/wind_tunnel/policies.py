"""Context-policy stubs for the wind-tunnel MVP.

These are not live model policies. They deterministically transform a recorded
action sequence so the harness can prove identity vs divergence gates without
multi-hour model replays. Swap real policies in later behind the same interface.
"""
from __future__ import annotations

from typing import Callable, Sequence

from benchmarks.wind_tunnel.types import Action

PolicyFn = Callable[[Sequence[Action]], list[Action]]


def identity(actions: Sequence[Action]) -> list[Action]:
    """Baseline: emit the recorded sequence unchanged."""
    return [Action(**a.__dict__) for a in actions]


def drop_shell(actions: Sequence[Action]) -> list[Action]:
    """Candidate stub: drop shell ops (proxy for eliding shell context mass)."""
    return [Action(**a.__dict__) for a in actions if a.method != "zero.token.shell"]


def collapse_compact_many(actions: Sequence[Action]) -> list[Action]:
    """Candidate stub: rewrite compactMany -> compact (forces method divergence)."""
    out: list[Action] = []
    for a in actions:
        method = (
            "zero.token.compact"
            if a.method == "zero.token.compactMany"
            else a.method
        )
        out.append(Action(index=a.index, id=a.id, method=method, state=a.state))
    return out


POLICIES: dict[str, PolicyFn] = {
    "identity": identity,
    "drop_shell": drop_shell,
    "collapse_compact_many": collapse_compact_many,
}


def get_policy(name: str) -> PolicyFn:
    try:
        return POLICIES[name]
    except KeyError as exc:
        known = ", ".join(sorted(POLICIES))
        raise SystemExit(f"unknown policy {name!r}; known: {known}") from exc
