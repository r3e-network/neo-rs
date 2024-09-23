use std::sync::atomic::{AtomicBool, Ordering};
use tungstenite::Message as WebSocketMessage;
use crate::neorpc::{self, EventID, Notification, SubscriptionFilter};

// intEvent is an internal event that has both a proper structure and
// a websocket-ready message. It's used to serve websocket-based clients
// as well as internal ones.
struct IntEvent {
    msg: WebSocketMessage,
    ntf: Notification,
}

// subscriber is an event subscriber.
struct Subscriber {
    writer: crossbeam_channel::Sender<IntEvent>,
    overflown: AtomicBool,
    // These work like slots as there is not a lot of them (it's
    // cheaper doing it this way rather than creating a map),
    // pointing to an EventID is an obvious overkill at the moment, but
    // that's not for long.
    feeds: [Feed; MAX_FEEDS],
}

// feed stores subscriber's desired event ID with filter.
struct Feed {
    event: EventID,
    filter: SubscriptionFilter,
}

impl Feed {
    // EventID implements neorpc::EventComparator trait and returns notification ID.
    fn event_id(&self) -> EventID {
        self.event
    }

    // Filter implements neorpc::EventComparator trait and returns notification filter.
    fn filter(&self) -> &SubscriptionFilter {
        &self.filter
    }
}

const MAX_FEEDS: usize = 16;

// This sets notification messages buffer depth. It may seem to be quite
// big, but there is a big gap in speed between internal event processing
// and networking communication that is combined with spiky nature of our
// event generation process, which leads to lots of events generated in
// a short time and they will put some pressure to this buffer (consider
// ~500 invocation txs in one block with some notifications). At the same
// time, this channel is about sending pointers, so it's doesn't cost
// a lot in terms of memory used.
const NOTIFICATION_BUF_SIZE: usize = 1024;
