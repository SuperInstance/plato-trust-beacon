//! plato-trust-beacon — BeaconLayer for trust event emission and observation
//!
//! Ships emit trust events (signals about other ships' reliability).
//! Observers collect these events and build consensus about trust levels.
//! Matches plato-ship-protocol::BeaconLayer trait.
//!
//! Sprint 3 Task S3-5: trust event propagation across the fleet.

use std::collections::HashSet;

// ── Beacon Trait ─────────────────────────────────────────

/// Beacon layer: trust event emission and consensus.
/// Matches plato-ship-protocol::BeaconLayer exactly.
pub trait BeaconLayer {
    fn emit_event(&mut self, event: &str, target: &str, strength: f32) -> bool;
    fn observe_events(&self, target: &str) -> Vec<TrustEvent>;
    fn consensus(&self, target: &str) -> f32;
}

// ── Trust Event ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TrustEvent {
    pub event_type: String,    // "success", "failure", "timeout", "corruption", "resurrect"
    pub emitter: String,       // who sent the signal
    pub target: String,        // who the signal is about
    pub strength: f32,         // 0.0-1.0, how strong the signal
    pub timestamp: u64,        // nanosecond
    pub generation: u32,       // beacon generation (for decay)
    pub decayed: bool,         // has this event decayed below threshold?
}

impl TrustEvent {
    pub fn new(emitter: &str, target: &str, event_type: &str, strength: f32) -> Self {
        Self {
            event_type: event_type.to_string(),
            emitter: emitter.to_string(),
            target: target.to_string(),
            strength: strength.max(-1.0).min(1.0),
            timestamp: nanos_now(),
            generation: 0,
            decayed: false,
        }
    }

    pub fn success(emitter: &str, target: &str) -> Self {
        Self::new(emitter, target, "success", 0.8)
    }

    pub fn failure(emitter: &str, target: &str) -> Self {
        Self::new(emitter, target, "failure", -0.9)
    }

    pub fn timeout(emitter: &str, target: &str) -> Self {
        Self::new(emitter, target, "timeout", -0.5)
    }

    pub fn corruption(emitter: &str, target: &str) -> Self {
        Self::new(emitter, target, "corruption", -0.95)
    }

    pub fn resurrect(emitter: &str, target: &str) -> Self {
        Self::new(emitter, target, "resurrect", 0.6)
    }

    /// Apply exponential decay. factor=0.9 means 10% reduction per generation.
    pub fn decay(&mut self, factor: f32) {
        self.generation += 1;
        self.strength *= factor;
        if self.strength.abs() < 0.05 {
            self.decayed = true;
        }
    }

    /// Absolute strength (magnitude regardless of sign)
    pub fn magnitude(&self) -> f32 {
        self.strength.abs()
    }

    /// Is this a negative signal?
    pub fn is_negative(&self) -> bool {
        self.strength < 0.0
    }
}

// ── Trust Beacon ─────────────────────────────────────────

/// Emits and observes trust events across the fleet.
/// Builds consensus trust scores from aggregated observations.
#[derive(Debug, Clone)]
pub struct TrustBeacon {
    events: Vec<TrustEvent>,
    max_events: usize,
    decay_factor: f32,
    consensus_threshold: usize, // minimum observations for consensus
    total_emitted: u64,
    total_decayed: u64,
}

impl TrustBeacon {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            max_events: 10_000,
            decay_factor: 0.9,
            consensus_threshold: 3,
            total_emitted: 0,
            total_decayed: 0,
        }
    }

    pub fn with_decay_factor(mut self, f: f32) -> Self {
        self.decay_factor = f;
        self
    }

    pub fn with_consensus_threshold(mut self, t: usize) -> Self {
        self.consensus_threshold = t;
        self
    }

    /// Emit a trust event with explicit strength
    pub fn emit(&mut self, emitter: &str, target: &str, event_type: &str, strength: f32) -> bool {
        if self.events.len() >= self.max_events {
            return false;
        }
        let event = TrustEvent::new(emitter, target, event_type, strength);
        self.events.push(event);
        self.total_emitted += 1;
        true
    }

    /// Observe all events about a target
    pub fn observe(&self, target: &str) -> Vec<&TrustEvent> {
        self.events.iter().filter(|e| e.target == target && !e.decayed).collect()
    }

    /// Compute consensus trust for a target (average of observed strengths)
    pub fn compute_consensus(&self, target: &str) -> f32 {
        let observed: Vec<&TrustEvent> = self.observe(target);
        if observed.len() < self.consensus_threshold {
            return 0.5; // no consensus → neutral
        }
        let sum: f32 = observed.iter().map(|e| e.strength).sum();
        let avg = sum / observed.len() as f32;
        // Clamp to 0.0-1.0
        (avg + 1.0) / 2.0 // map [-1,1] → [0,1]
    }

    /// Decay all events by one generation
    pub fn decay_all(&mut self) -> usize {
        let before = self.events.len();
        for event in &mut self.events {
            event.decay(self.decay_factor);
        }
        self.events.retain(|e| !e.decayed);
        let removed = before - self.events.len();
        self.total_decayed += removed as u64;
        removed
    }

    /// Prune events about a specific target
    pub fn prune_target(&mut self, target: &str) -> usize {
        let before = self.events.len();
        self.events.retain(|e| e.target != target);
        before - self.events.len()
    }

    /// Get all unique targets with events
    pub fn known_targets(&self) -> HashSet<String> {
        self.events.iter().map(|e| e.target.clone()).collect()
    }

    /// Event count for a target
    pub fn event_count(&self, target: &str) -> usize {
        self.events.iter().filter(|e| e.target == target).count()
    }

    /// Stats
    pub fn stats(&self) -> BeaconStats {
        BeaconStats {
            total_events: self.events.len(),
            total_emitted: self.total_emitted,
            total_decayed: self.total_decayed,
            known_targets: self.known_targets().len(),
        }
    }

    /// Emit multiple events (batch)
    pub fn emit_batch(&mut self, events: Vec<TrustEvent>) -> usize {
        let mut count = 0;
        for event in events {
            if self.events.len() < self.max_events {
                self.events.push(event);
                self.total_emitted += 1;
                count += 1;
            }
        }
        count
    }

    /// Propagate: emit events from one beacon into another (for multi-ship)
    pub fn propagate_from(&mut self, other: &TrustBeacon) -> usize {
        let events: Vec<TrustEvent> = other.events.iter().cloned().collect();
        self.emit_batch(events)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BeaconStats {
    pub total_events: usize,
    pub total_emitted: u64,
    pub total_decayed: u64,
    pub known_targets: usize,
}

impl Default for TrustBeacon {
    fn default() -> Self { Self::new() }
}

impl BeaconLayer for TrustBeacon {
    fn emit_event(&mut self, event: &str, target: &str, strength: f32) -> bool {
        self.emit("fleet", target, event, strength)
    }

    fn observe_events(&self, target: &str) -> Vec<TrustEvent> {
        self.observe(target).into_iter().cloned().collect()
    }

    fn consensus(&self, target: &str) -> f32 {
        self.compute_consensus(target)
    }
}

fn nanos_now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0)
}

// ── Tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_observe() {
        let mut beacon = TrustBeacon::new();
        beacon.emit("oracle1", "jc1", "success", 0.8);
        let events = beacon.observe("jc1");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].emitter, "oracle1");
    }

    #[test]
    fn test_conensus_neutral() {
        let beacon = TrustBeacon::new();
        // No events about unknown target → neutral
        assert_eq!(beacon.compute_consensus("nobody"), 0.5);
    }

    #[test]
    fn test_conensus_positive() {
        let mut beacon = TrustBeacon::new();
        beacon.emit("a", "target", "success", 0.8);
        beacon.emit("b", "target", "success", 0.9);
        beacon.emit("c", "target", "success", 0.7);
        // 3 observations meet threshold, all positive
        let consensus = beacon.compute_consensus("target");
        assert!(consensus > 0.8); // should be high
    }

    #[test]
    fn test_conensus_negative() {
        let mut beacon = TrustBeacon::new();
        beacon.emit("a", "bad_ship", "failure", -0.9);
        beacon.emit("b", "bad_ship", "corruption", -0.95);
        beacon.emit("c", "bad_ship", "timeout", -0.5);
        let consensus = beacon.compute_consensus("bad_ship");
        assert!(consensus < 0.3); // should be low
    }

    #[test]
    fn test_conensus_mixed() {
        let mut beacon = TrustBeacon::new();
        beacon.emit("a", "mixed", "success", 0.8);
        beacon.emit("b", "mixed", "failure", -0.9);
        beacon.emit("c", "mixed", "success", 0.7);
        // Mixed signals → mid-range
        let consensus = beacon.compute_consensus("mixed");
        assert!(consensus > 0.3 && consensus < 0.7);
    }

    #[test]
    fn test_decay() {
        let mut event = TrustEvent::success("a", "b");
        assert_eq!(event.generation, 0);
        event.decay(0.5);
        assert_eq!(event.generation, 1);
        assert!((event.strength - 0.4).abs() < 0.01);
    }

    #[test]
    fn test_decay_to_zero() {
        let mut event = TrustEvent::success("a", "b");
        event.decay(0.01); // gen 1, strength 0.008
        assert!(event.decayed); // below 0.05 threshold
    }

    #[test]
    fn test_decay_all() {
        let mut beacon = TrustBeacon::new().with_decay_factor(0.5);
        beacon.emit("a", "b", "success", 0.8);
        beacon.emit("a", "c", "success", 0.8);
        beacon.emit("a", "d", "success", 0.8);

        // First decay: 0.4 each, still alive
        let removed1 = beacon.decay_all();
        assert_eq!(removed1, 0);
        assert_eq!(beacon.events.len(), 3);

        // Second decay: 0.2 each, still alive
        let removed2 = beacon.decay_all();
        assert_eq!(removed2, 0);

        // Third decay: 0.1 each, still alive
        let removed3 = beacon.decay_all();
        assert_eq!(removed3, 0);

        // Fourth decay: 0.05 each, still barely alive
        let removed4 = beacon.decay_all();
        assert_eq!(removed4, 0);

        // Fifth decay: 0.025, decayed
        let removed5 = beacon.decay_all();
        assert_eq!(removed5, 3);
        assert!(beacon.events.is_empty());
    }

    #[test]
    fn test_known_targets() {
        let mut beacon = TrustBeacon::new();
        beacon.emit("a", "oracle1", "success", 0.8);
        beacon.emit("a", "jc1", "success", 0.8);
        beacon.emit("a", "oracle1", "failure", -0.5);

        let targets = beacon.known_targets();
        assert!(targets.contains("oracle1"));
        assert!(targets.contains("jc1"));
        assert_eq!(targets.len(), 2);
    }

    #[test]
    fn test_prune_target() {
        let mut beacon = TrustBeacon::new();
        beacon.emit("a", "oracle1", "success", 0.8);
        beacon.emit("a", "jc1", "success", 0.8);
        beacon.emit("a", "oracle1", "failure", -0.5);

        let pruned = beacon.prune_target("oracle1");
        assert_eq!(pruned, 2);
        assert_eq!(beacon.event_count("oracle1"), 0);
        assert_eq!(beacon.event_count("jc1"), 1);
    }

    #[test]
    fn test_capacity_limit() {
        let mut beacon = TrustBeacon::new();
        beacon.max_events = 2;
        assert!(beacon.emit("a", "b", "success", 0.8));
        assert!(beacon.emit("a", "c", "success", 0.8));
        assert!(!beacon.emit("a", "d", "success", 0.8)); // over capacity
    }

    #[test]
    fn test_batch_emit() {
        let mut beacon = TrustBeacon::new();
        let events = vec![
            TrustEvent::success("a", "b"),
            TrustEvent::failure("a", "c"),
            TrustEvent::timeout("a", "d"),
        ];
        let count = beacon.emit_batch(events);
        assert_eq!(count, 3);
        assert_eq!(beacon.events.len(), 3);
    }

    #[test]
    fn test_propagate() {
        let mut beacon_a = TrustBeacon::new();
        beacon_a.emit("ship_a", "target", "success", 0.9);

        let mut beacon_b = TrustBeacon::new();
        let count = beacon_b.propagate_from(&beacon_a);
        assert_eq!(count, 1);
        assert_eq!(beacon_b.event_count("target"), 1);
    }

    #[test]
    fn test_beacon_layer_trait() {
        let mut beacon = TrustBeacon::new();
        assert!(beacon.emit_event("success", "target", 0.8));

        let events = beacon.observe_events("target");
        assert_eq!(events.len(), 1);

        // Need 3 events for consensus
        beacon.emit_event("success", "target", 0.9);
        beacon.emit_event("success", "target", 0.7);
        let c = beacon.consensus("target");
        assert!(c > 0.8);
    }

    #[test]
    fn test_event_types() {
        let s = TrustEvent::success("a", "b");
        assert_eq!(s.event_type, "success");
        assert!(!s.is_negative());

        let f = TrustEvent::failure("a", "b");
        assert_eq!(f.event_type, "failure");
        assert!(f.is_negative());

        let t = TrustEvent::timeout("a", "b");
        assert_eq!(t.event_type, "timeout");
        assert!(t.is_negative());

        let c = TrustEvent::corruption("a", "b");
        assert_eq!(c.event_type, "corruption");
        assert!(c.is_negative());

        let r = TrustEvent::resurrect("a", "b");
        assert_eq!(r.event_type, "resurrect");
        assert!(!r.is_negative());
    }

    #[test]
    fn test_strength_clamping() {
        let event = TrustEvent::new("a", "b", "test", 2.0);
        assert_eq!(event.strength, 1.0);
        let neg = TrustEvent::new("a", "b", "test", -2.0);
        assert_eq!(neg.strength, -1.0);
    }

    #[test]
    fn test_magnitude() {
        let pos = TrustEvent::new("a", "b", "test", 0.8);
        assert!((pos.magnitude() - 0.8).abs() < 0.01);
        let neg = TrustEvent::new("a", "b", "test", -0.9);
        assert!((neg.magnitude() - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_stats() {
        let mut beacon = TrustBeacon::new();
        beacon.emit("a", "b", "success", 0.8);
        beacon.emit("a", "c", "success", 0.8);

        let stats = beacon.stats();
        assert_eq!(stats.total_events, 2);
        assert_eq!(stats.total_emitted, 2);
        assert_eq!(stats.total_decayed, 0);
        assert_eq!(stats.known_targets, 2);
    }

    #[test]
    fn test_conensus_threshold_custom() {
        let mut beacon = TrustBeacon::new().with_consensus_threshold(5);
        beacon.events.push(TrustEvent::success("a", "t"));
        beacon.events.push(TrustEvent::success("b", "t"));
        beacon.events.push(TrustEvent::success("c", "t"));
        // Only 3 events, threshold is 5 → no consensus
        assert_eq!(beacon.compute_consensus("t"), 0.5);
    }
}
