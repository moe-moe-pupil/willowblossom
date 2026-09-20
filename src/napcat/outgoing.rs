use std::{
    collections::BTreeMap,
    future::pending,
    time::Duration,
};

use rand::{
    rng,
    RngExt,
};
use serde_json::{
    from_str,
    Value,
};
use tokio::{
    sync::mpsc::Receiver,
    time::{
        sleep_until,
        Instant,
    },
};

use super::NapcatOutboundMessage;

fn recipient(message: &NapcatOutboundMessage) -> Option<(String, String)> {
    let payload: Value = from_str(message.message.to_text().ok()?).ok()?;
    let action = payload["action"].as_str()?;
    let key = match action {
        "send_private_msg" => "user_id",
        "send_group_msg" => "group_id",
        _ => return None,
    };
    let id = &payload["params"][key];
    let id = id
        .as_str()
        .map(str::to_owned)
        .or_else(|| id.as_u64().map(|id| id.to_string()))?;
    Some((action.to_owned(), id))
}

/// Separate deadlines prevent a broadcast's recipients from waiting in series.
#[derive(Default)]
pub(super) struct OutgoingQueue {
    messages: BTreeMap<(Instant, u64), NapcatOutboundMessage>,
    sequence: u64,
}

impl OutgoingQueue {
    fn schedule(&mut self, message: NapcatOutboundMessage, now: Instant, delay: Duration) {
        let mut deadline = now + delay;
        let destination = recipient(&message);
        // Preserve message order within a conversation without delaying other recipients.
        for ((queued_deadline, _), queued) in &self.messages {
            if destination.is_some() && recipient(queued) == destination {
                deadline = deadline.max(*queued_deadline);
            }
        }
        self.messages.insert((deadline, self.sequence), message);
        self.sequence += 1;
    }

    pub(super) async fn recv(
        &mut self,
        receiver: &mut Receiver<NapcatOutboundMessage>,
    ) -> Option<NapcatOutboundMessage> {
        loop {
            let deadline = self.messages.first_key_value().map(|(key, _)| key.0);
            tokio::select! {
                message = receiver.recv() => {
                    if let Some(message) = message {
                        let delay = Duration::from_millis(rng().random_range(1_000..=2_000));
                        self.schedule(message, Instant::now(), delay);
                    } else {
                        if let Some(deadline) = deadline {
                            sleep_until(deadline).await;
                        }
                        return self.messages.pop_first().map(|(_, message)| message);
                    }
                }
                _ = async {
                    match deadline {
                        Some(deadline) => sleep_until(deadline).await,
                        None => pending::<()>().await,
                    }
                } => return self.messages.pop_first().map(|(_, message)| message),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::{
        sync::mpsc::channel,
        time::timeout,
    };
    use tokio_tungstenite::tungstenite::protocol::Message;

    use super::*;

    fn message(id: u64, target: &str) -> NapcatOutboundMessage {
        NapcatOutboundMessage {
            request_id: id,
            target_id: "shared-broadcast-input".to_owned(),
            message: Message::Text(format!(r#"{{"action":"send_private_msg","params":{{"user_id":"{target}","message":"test"}}}}"#).into()),
        }
    }

    #[test]
    fn recipients_have_independent_deadlines_and_conversations_stay_ordered() {
        let mut queue = OutgoingQueue::default();
        let now = Instant::now();
        queue.schedule(
            message(1, "B"),
            now,
            Duration::from_millis(1_400),
        );
        queue.schedule(
            message(2, "C"),
            now,
            Duration::from_millis(1_200),
        );
        queue.schedule(
            message(3, "B"),
            now,
            Duration::from_millis(1_000),
        );
        let deliveries = queue
            .messages
            .into_iter()
            .map(|((at, _), message)| {
                (
                    at.duration_since(now).as_millis(),
                    message.request_id,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(deliveries, [
            (1_200, 2),
            (1_400, 1),
            (1_400, 3)
        ]);
    }

    #[tokio::test]
    async fn cancellation_preserves_delays_and_closed_channel_drains_messages() {
        let (sender, mut receiver) = channel(2);
        sender.send(message(1, "B")).await.unwrap();
        sender.send(message(2, "C")).await.unwrap();
        let start = Instant::now();
        let mut queue = OutgoingQueue::default();
        assert!(timeout(
            Duration::from_millis(20),
            queue.recv(&mut receiver)
        )
        .await
        .is_err());
        assert_eq!(queue.messages.len(), 2);
        for (deadline, _) in queue.messages.keys() {
            let delay = deadline.duration_since(start);
            assert!(delay >= Duration::from_secs(1));
            assert!(delay <= Duration::from_millis(2_020));
        }
        drop(sender);
        let first = queue.recv(&mut receiver).await.unwrap();
        assert!(start.elapsed() >= Duration::from_secs(1));
        let second = queue.recv(&mut receiver).await.unwrap();
        assert_ne!(first.request_id, second.request_id);
        assert!(queue.recv(&mut receiver).await.is_none());
    }
}
