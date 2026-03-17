use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct Stats {
    connections_total: AtomicU64,
    connections_ws: AtomicU64,
    bytes_up: AtomicU64,
    bytes_down: AtomicU64,
}

impl Stats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn inc_total(&self) {
        self.connections_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_ws(&self) {
        self.connections_ws.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_bytes_up(&self, n: u64) {
        self.bytes_up.fetch_add(n, Ordering::Relaxed);
    }

    pub fn add_bytes_down(&self, n: u64) {
        self.bytes_down.fetch_add(n, Ordering::Relaxed);
    }
}
