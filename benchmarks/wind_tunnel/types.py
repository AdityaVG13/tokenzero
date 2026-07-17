"""Shared types for the wind-tunnel replay MVP."""
from __future__ import annotations

from dataclasses import asdict, dataclass
from typing import Any


@dataclass(frozen=True)
class Action:
    """One recorded plan-journal operation treated as an action atom."""

    index: int
    id: str
    method: str
    state: str = ""

    def fingerprint(self) -> tuple[int, str, str]:
        """Compare identity without requiring journal state equality."""
        return (self.index, self.id, self.method)

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


@dataclass(frozen=True)
class SequenceDiff:
    journal: str
    baseline: list[Action]
    candidate: list[Action]
    first_divergence: int | None

    @property
    def match(self) -> bool:
        return self.first_divergence is None

    def to_dict(self) -> dict[str, Any]:
        return {
            "journal": self.journal,
            "match": self.match,
            "first_divergence": self.first_divergence,
            "baseline_len": len(self.baseline),
            "candidate_len": len(self.candidate),
            "baseline": [a.to_dict() for a in self.baseline],
            "candidate": [a.to_dict() for a in self.candidate],
        }
