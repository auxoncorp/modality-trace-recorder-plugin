use crate::attr::{AttrKeys, EventAttrKey, TimelineAttrKey};
use auxon_sdk::{
    ingest_client::{BoundTimelineState, IngestClient, IngestError},
    ingest_protocol::InternedAttrKey,
};

pub struct Client {
    timeline_keys: AttrKeys<TimelineAttrKey>,
    event_keys: AttrKeys<EventAttrKey>,
    inner: IngestClient<BoundTimelineState>,
}

impl Client {
    pub fn new(client: IngestClient<BoundTimelineState>) -> Self {
        Self {
            timeline_keys: AttrKeys::default(),
            event_keys: AttrKeys::default(),
            inner: client,
        }
    }

    pub async fn close(mut self) -> Result<(), IngestError> {
        self.inner.flush().await?;
        let _ = self.inner.close_timeline();
        Ok(())
    }

    pub async fn timeline_key(
        &mut self,
        key: TimelineAttrKey,
    ) -> Result<InternedAttrKey, IngestError> {
        let k = self.timeline_keys.get(&mut self.inner, key).await?;
        Ok(k)
    }

    pub async fn event_key(&mut self, key: EventAttrKey) -> Result<InternedAttrKey, IngestError> {
        let k = self.event_keys.get(&mut self.inner, key).await?;
        Ok(k)
    }

    pub fn inner(&mut self) -> &mut IngestClient<BoundTimelineState> {
        &mut self.inner
    }

    pub(crate) fn remove_timeline_string_key(
        &mut self,
        key: &str,
    ) -> Option<(TimelineAttrKey, InternedAttrKey)> {
        self.timeline_keys.remove_string_key_entry(key)
    }

    pub(crate) fn add_timeline_key(&mut self, key: TimelineAttrKey, interned_key: InternedAttrKey) {
        self.timeline_keys.insert(key, interned_key)
    }
}
