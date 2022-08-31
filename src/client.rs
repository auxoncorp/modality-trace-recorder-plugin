use crate::attr::{AttrKeyIndex, AttrKeys};
use modality_ingest_client::{BoundTimelineState, IngestClient, IngestError};
use modality_ingest_protocol::InternedAttrKey;

pub struct Client<TAK: AttrKeyIndex, EAK: AttrKeyIndex> {
    timeline_keys: AttrKeys<TAK>,
    event_keys: AttrKeys<EAK>,
    inner: IngestClient<BoundTimelineState>,
}

impl<TAK: AttrKeyIndex, EAK: AttrKeyIndex> Client<TAK, EAK> {
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

    pub async fn timeline_key<K: Into<TAK>>(
        &mut self,
        key: K,
    ) -> Result<InternedAttrKey, IngestError> {
        let k = self.timeline_keys.get(&mut self.inner, key.into()).await?;
        Ok(k)
    }

    pub async fn event_key<K: Into<EAK>>(
        &mut self,
        key: K,
    ) -> Result<InternedAttrKey, IngestError> {
        let k = self.event_keys.get(&mut self.inner, key.into()).await?;
        Ok(k)
    }

    pub fn inner(&mut self) -> &mut IngestClient<BoundTimelineState> {
        &mut self.inner
    }
}
