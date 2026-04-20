"""Trust beacon for cross-agent trust signaling."""

import time
from dataclasses import dataclass, field
from enum import Enum

class TrustLevel(Enum):
    UNKNOWN = 0
    LOW = 0.25
    MEDIUM = 0.5
    HIGH = 0.75
    CRITICAL = 1.0

@dataclass
class TrustSignal:
    source: str
    target: str
    level: float
    domain: str = ""
    timestamp: float = field(default_factory=time.time)
    evidence: str = ""

class TrustBeacon:
    def __init__(self, agent_id: str):
        self.agent_id = agent_id
        self._signals: dict[str, list[TrustSignal]] = {}
        self._trust_scores: dict[str, float] = {}

    def emit(self, target: str, level: float, domain: str = "", evidence: str = "") -> TrustSignal:
        sig = TrustSignal(source=self.agent_id, target=target, level=max(0, min(level, 1)),
                          domain=domain, evidence=evidence)
        if target not in self._signals:
            self._signals[target] = []
        self._signals[target].append(sig)
        self._trust_scores[target] = self._aggregate(target)
        return sig

    def receive(self, signal: TrustSignal):
        if signal.target not in self._signals:
            self._signals[signal.target] = []
        self._signals[signal.target].append(signal)
        self._trust_scores[signal.target] = self._aggregate(signal.target)

    def get_trust(self, agent_id: str) -> float:
        return self._trust_scores.get(agent_id, 0.0)

    def _aggregate(self, target: str) -> float:
        sigs = self._signals.get(target, [])
        if not sigs: return 0.0
        total, weight = 0.0, 0.0
        for s in sigs:
            w = 0.99 ** ((time.time() - s.timestamp) / 3600)
            total += s.level * w
            weight += w
        return total / max(weight, 1e-9)

    def top_trusted(self, n: int = 10) -> list[tuple[str, float]]:
        scores = [(k, v) for k, v in self._trust_scores.items()]
        scores.sort(key=lambda x: -x[1])
        return scores[:n]

    def domain_trust(self, domain: str) -> dict[str, float]:
        result = {}
        for agent, sigs in self._signals.items():
            domain_sigs = [s for s in sigs if s.domain == domain]
            if domain_sigs:
                result[agent] = sum(s.level for s in domain_sigs) / len(domain_sigs)
        return dict(sorted(result.items(), key=lambda x: -x[1]))

    @property
    def stats(self) -> dict:
        levels = {}
        for a, s in self._trust_scores.items():
            if s >= 0.75: levels["high"] = levels.get("high", 0) + 1
            elif s >= 0.5: levels["medium"] = levels.get("medium", 0) + 1
            elif s >= 0.25: levels["low"] = levels.get("low", 0) + 1
            else: levels["unknown"] = levels.get("unknown", 0) + 1
        return {"agents_tracked": len(self._trust_scores), "levels": levels,
                "signals_received": sum(len(v) for v in self._signals.values())}
