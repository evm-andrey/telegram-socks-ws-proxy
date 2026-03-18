use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const WS_FAILURE_COOLDOWN: Duration = Duration::from_secs(60);
const WS_ATTEMPT_WINDOW: Duration = Duration::from_secs(15);

type WsKey = (u8, bool);

static WS_BACKOFF: Lazy<Mutex<WsBackoff>> = Lazy::new(|| Mutex::new(WsBackoff::default()));

#[derive(Debug, Default)]
struct WsBackoff {
    states: HashMap<WsKey, WsState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WsState {
    Probing(Instant),
    Cooldown(Instant),
    Disabled404,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WsDecision {
    ProbeNow,
    ProbeInFlight,
    Cooldown,
    Disabled404,
}

impl WsBackoff {
    fn begin_probe(&mut self, key: WsKey, now: Instant) -> WsDecision {
        self.states.retain(|_, state| match state {
            WsState::Probing(until) | WsState::Cooldown(until) => *until > now,
            WsState::Disabled404 => true,
        });

        match self.states.get(&key).copied() {
            Some(WsState::Probing(_)) => WsDecision::ProbeInFlight,
            Some(WsState::Cooldown(_)) => WsDecision::Cooldown,
            Some(WsState::Disabled404) => WsDecision::Disabled404,
            None => {
                self.states
                    .insert(key, WsState::Probing(now + WS_ATTEMPT_WINDOW));
                WsDecision::ProbeNow
            }
        }
    }

    fn record_failure(&mut self, key: WsKey, now: Instant) {
        self.states
            .insert(key, WsState::Cooldown(now + WS_FAILURE_COOLDOWN));
    }

    fn disable_404(&mut self, key: WsKey) {
        self.states.insert(key, WsState::Disabled404);
    }

    fn clear(&mut self, key: WsKey) {
        self.states.remove(&key);
    }
}

pub(super) fn begin_ws_probe(key: WsKey, now: Instant) -> WsDecision {
    WS_BACKOFF
        .lock()
        .expect("ws backoff lock")
        .begin_probe(key, now)
}

pub(super) fn record_ws_failure(key: WsKey, now: Instant) {
    WS_BACKOFF
        .lock()
        .expect("ws backoff lock")
        .record_failure(key, now);
}

pub(super) fn disable_ws_route(key: WsKey) {
    WS_BACKOFF.lock().expect("ws backoff lock").disable_404(key);
}

pub(super) fn clear_ws_probe(key: WsKey) {
    WS_BACKOFF.lock().expect("ws backoff lock").clear(key);
}

#[cfg(test)]
mod tests {
    use super::{WsBackoff, WsDecision};
    use std::time::{Duration, Instant};

    #[test]
    fn ws_backoff_expires_after_cooldown() {
        let mut backoff = WsBackoff::default();
        let key = (2, false);
        let now = Instant::now();

        assert_eq!(backoff.begin_probe(key, now), WsDecision::ProbeNow);
        backoff.record_failure(key, now);
        assert_eq!(
            backoff.begin_probe(key, now + Duration::from_secs(4)),
            WsDecision::Cooldown
        );
        assert_eq!(
            backoff.begin_probe(key, now + Duration::from_secs(60)),
            WsDecision::ProbeNow
        );
    }

    #[test]
    fn ws_backoff_can_be_cleared() {
        let mut backoff = WsBackoff::default();
        let key = (4, true);
        let now = Instant::now();

        backoff.record_failure(key, now);
        assert_eq!(
            backoff.begin_probe(key, now + Duration::from_secs(1)),
            WsDecision::Cooldown
        );
        backoff.clear(key);
        assert_eq!(
            backoff.begin_probe(key, now + Duration::from_secs(1)),
            WsDecision::ProbeNow
        );
    }

    #[test]
    fn ws_backoff_blocks_parallel_probe_window() {
        let mut backoff = WsBackoff::default();
        let key = (2, true);
        let now = Instant::now();

        assert_eq!(backoff.begin_probe(key, now), WsDecision::ProbeNow);
        assert_eq!(
            backoff.begin_probe(key, now + Duration::from_secs(1)),
            WsDecision::ProbeInFlight
        );
    }

    #[test]
    fn ws_backoff_can_disable_404_for_runtime() {
        let mut backoff = WsBackoff::default();
        let key = (5, false);
        let now = Instant::now();

        backoff.disable_404(key);
        assert_eq!(backoff.begin_probe(key, now), WsDecision::Disabled404);
    }
}
