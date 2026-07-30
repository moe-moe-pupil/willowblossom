use bevy::prelude::Resource;
use serde::{
    Deserialize,
    Serialize,
};

pub(super) const NAPCAT_INBOUND_RECEIPTS_PATH: &str = ".data/willowblossom/inbound_receipts.toml";

/// NapCat keeps roughly 5,000 messages in its own LRU cache. Keeping more receipts than that
/// ensures any event it can resend remains recognizable even after local chat history is deleted.
pub(super) const MAX_INBOUND_MESSAGE_RECEIPTS: usize = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct InboundMessageReceipt {
    self_id: u64,
    message_id: i64,
}

#[derive(Debug, Default, Resource, Serialize, Deserialize)]
pub(super) struct InboundMessageReceiptStore {
    #[serde(default)]
    receipts: Vec<InboundMessageReceipt>,
}

impl InboundMessageReceiptStore {
    pub(super) fn contains(&self, self_id: u64, message_id: Option<i64>) -> bool {
        let Some(receipt) = receipt(self_id, message_id) else {
            return false;
        };
        self.receipts.contains(&receipt)
    }

    pub(super) fn record(&mut self, self_id: u64, message_id: Option<i64>) -> bool {
        let Some(receipt) = receipt(self_id, message_id) else {
            return false;
        };
        if self.receipts.contains(&receipt) {
            return false;
        }

        self.receipts.push(receipt);
        let excess = self
            .receipts
            .len()
            .saturating_sub(MAX_INBOUND_MESSAGE_RECEIPTS);
        if excess > 0 {
            self.receipts.drain(..excess);
        }
        true
    }

    pub(super) fn is_empty(&self) -> bool { self.receipts.is_empty() }

    #[cfg(test)]
    fn len(&self) -> usize { self.receipts.len() }
}

fn receipt(self_id: u64, message_id: Option<i64>) -> Option<InboundMessageReceipt> {
    Some(InboundMessageReceipt {
        self_id,
        message_id: message_id.filter(|id| *id > 0)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipts_are_scoped_to_the_napcat_account_and_round_trip() {
        let mut store = InboundMessageReceiptStore::default();
        assert!(store.record(10, Some(7001)));
        assert!(!store.record(10, Some(7001)));
        assert!(!store.record(10, None));
        assert!(!store.record(10, Some(0)));

        let encoded = serde_json::to_string(&store).unwrap();
        let restored = serde_json::from_str::<InboundMessageReceiptStore>(&encoded).unwrap();
        assert!(restored.contains(10, Some(7001)));
        assert!(!restored.contains(11, Some(7001)));
    }

    #[test]
    fn receipts_keep_more_entries_than_napcat_can_resend() {
        let mut store = InboundMessageReceiptStore::default();
        for message_id in 1..=MAX_INBOUND_MESSAGE_RECEIPTS as i64 + 1 {
            assert!(store.record(10, Some(message_id)));
        }

        assert_eq!(
            store.len(),
            MAX_INBOUND_MESSAGE_RECEIPTS
        );
        assert!(!store.contains(10, Some(1)));
        assert!(store.contains(
            10,
            Some(MAX_INBOUND_MESSAGE_RECEIPTS as i64 + 1)
        ));
    }
}
