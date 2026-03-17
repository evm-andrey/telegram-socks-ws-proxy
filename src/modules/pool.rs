use crate::modules::ws::RawWsClient;
use std::collections::HashMap;

#[derive(Default)]
pub struct WsPool {
    _buckets: HashMap<(u8, bool), Vec<RawWsClient>>,
}

impl WsPool {
    pub fn new() -> Self {
        Self {
            _buckets: HashMap::new(),
        }
    }

    pub async fn get(&mut self, dc: u8, is_media: bool) -> Option<RawWsClient> {
        let key = (dc, is_media);
        self._buckets.get_mut(&key).and_then(|bucket| bucket.pop())
    }

    pub async fn return_to_pool(&mut self, dc: u8, is_media: bool, client: RawWsClient) {
        self._buckets
            .entry((dc, is_media))
            .or_default()
            .push(client);
    }
}
