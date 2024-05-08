use crate::{
    attr::{AttrKeys, EventAttrKey, TimelineAttrKey},
    error::Error,
};
use auxon_sdk::{
    api::{AttrVal, TimelineId},
    ingest_client::{dynamic::DynamicIngestClient, IngestClient, ReadyState},
};

pub struct Client {
    timeline_keys: AttrKeys<TimelineAttrKey>,
    event_keys: AttrKeys<EventAttrKey>,
    pub(crate) inner: DynamicIngestClient,
}

impl Client {
    pub fn new(client: IngestClient<ReadyState>) -> Self {
        Self {
            timeline_keys: AttrKeys::default(),
            event_keys: AttrKeys::default(),
            inner: client.into(),
        }
    }

    pub async fn switch_timeline(
        &mut self,
        id: TimelineId,
        new_timeline_attrs: Option<impl IntoIterator<Item = (&TimelineAttrKey, &AttrVal)>>,
    ) -> Result<(), Error> {
        self.inner.open_timeline(id).await?;
        if let Some(attrs) = new_timeline_attrs {
            let mut interned_attrs = Vec::new();
            for (k, v) in attrs.into_iter() {
                let int_key = if let Some(ik) = self.timeline_keys.get(k) {
                    *ik
                } else {
                    let ik = self.inner.declare_attr_key(k.to_string()).await?;
                    self.timeline_keys.insert(k.clone(), ik);
                    ik
                };
                interned_attrs.push((int_key, v.clone()));
            }
            self.inner.timeline_metadata(interned_attrs).await?;
        }
        Ok(())
    }

    pub async fn send_event(
        &mut self,
        ordering: u128,
        attrs: impl IntoIterator<Item = (&EventAttrKey, &AttrVal)>,
    ) -> Result<(), Error> {
        let mut interned_attrs = Vec::new();
        for (k, v) in attrs.into_iter() {
            let int_key = if let Some(ik) = self.event_keys.get(k) {
                *ik
            } else {
                let ik = self.inner.declare_attr_key(k.to_string()).await?;
                self.event_keys.insert(k.clone(), ik);
                ik
            };
            interned_attrs.push((int_key, v.clone()));
        }
        self.inner.event(ordering, interned_attrs).await?;
        Ok(())
    }
}
