//! Full std::collections based SessionManager implementation.
//!
//! This is intended to be the lowest-friction interface to get
//! started, both for library development and eventually for using the library.

use embedded_time::{duration::Duration, fixed_point::FixedPoint, Clock};

use crate::session::*;
use crate::types::NodeId;

use alloc::{collections::BTreeMap, vec::Vec};

/// Internal session object.
#[derive(Clone, Debug)]
struct Session<T, C>
where
    T: crate::transport::SessionMetadata<C>,
    C: Clock,
{
    // Timestamp of first frame
    pub timestamp: Option<Timestamp<C>>,
    pub payload: Vec<u8>,
    pub transfer_id: TransferId,

    pub md: T,
}

impl<T, C> Session<T, C>
where
    T: crate::transport::SessionMetadata<C>,
    C: Clock,
{
    pub fn new(transfer_id: TransferId, known_max_payload_size: Option<usize>) -> Self {
        Self {
            timestamp: None,
            payload: Vec::with_capacity(known_max_payload_size.unwrap_or(10)),
            transfer_id,
            md: T::new(),
        }
    }

    pub fn reset(&mut self) {
        self.payload.clear();
        self.timestamp = None;
        self.md = T::new()
    }

    pub fn reset_to_new_transfer_id(&mut self, transfer_id: TransferId) {
        self.reset();
        self.transfer_id = transfer_id;
    }
}

/// Internal subscription object. Contains hash map of sessions.
struct Subscription<T, D, C>
where
    T: crate::transport::SessionMetadata<C>,
    D: Duration + FixedPoint,
    C: Clock,
{
    sub: crate::Subscription<D>,
    sessions: BTreeMap<NodeId, Session<T, C>>,
}

impl<T, D, C> Subscription<T, D, C>
where
    T: crate::transport::SessionMetadata<C>,
    D: Duration + FixedPoint,
    C: Clock,
    <C as embedded_time::Clock>::T: From<<D as FixedPoint>::T>,
{
    pub fn new(sub: crate::Subscription<D>) -> Self {
        Self {
            sub,
            sessions: BTreeMap::new(),
        }
    }

    /// Update subscription with incoming frame
    fn update(&mut self, frame: InternalRxFrame<C>) -> Result<Option<Transfer<C>>, SessionError> {
        // TODO maybe some of the logic here can be skipped with anon transfers.
        let session_id = frame.source_node_id.unwrap();

        let extent = self.sub.extent;
        let session = self.sessions.get_mut(&session_id);
        match session {
            // error if session not exists and not start of transfer
            None if !frame.start_of_transfer => return Err(SessionError::NewSessionNoStart),
            // create new session if not exists (start of transfer)
            None => {
                self.sessions
                    .insert(session_id, Session::new(frame.transfer_id, Some(extent)));
            }
            // session already exists and check for wrong transfer_id
            Some(session) if session.transfer_id != frame.transfer_id => {
                session.reset_to_new_transfer_id(frame.transfer_id);
            }
            // session already exists and check for timeout
            Some(session)
                if timestamp_expired(self.sub.timeout, frame.timestamp, session.timestamp) =>
            {
                session.reset();
                return Err(SessionError::Timeout);
            }
            _ => (),
        }

        self.accept_frame(session_id, frame)
    }

    fn accept_frame(
        &mut self,
        session: NodeId,
        frame: InternalRxFrame<C>,
    ) -> Result<Option<Transfer<C>>, SessionError> {
        let mut session = self.sessions.get_mut(&session).unwrap();

        if frame.start_of_transfer {
            session.timestamp = Some(frame.timestamp);
        }

        if let Some(len) = session.md.update(&frame) {
            // Truncate payload if subscription extent is less than the incoming data
            let payload_to_copy = if session.payload.len() + len > self.sub.extent {
                session.payload.len() + len - self.sub.extent
            } else {
                len
            };
            session.payload.extend(&frame.payload[0..payload_to_copy]);

            if frame.end_of_transfer {
                if session.md.is_valid(&frame) {
                    Ok(Some(Transfer::from_frame(
                        frame,
                        session.timestamp.unwrap(),
                        &session.payload,
                    )))
                } else {
                    Err(SessionError::BadMetadata)
                }
            } else {
                Ok(None)
            }
        } else {
            Err(SessionError::BadMetadata)
        }
    }
}

/// SessionManager based on full std support. Meant to be lowest
/// barrier to entry and greatest flexibility at the cost of resource usage
/// and not being no_std.
pub struct HeapSessionManager<T, D, C>
where
    T: crate::transport::SessionMetadata<C>,
    D: Duration + FixedPoint,
    C: Clock,
{
    subscriptions: Vec<Subscription<T, D, C>>,
}

impl<T, D, C> HeapSessionManager<T, D, C>
where
    T: crate::transport::SessionMetadata<C>,
    C: Clock,
    D: embedded_time::duration::Duration + FixedPoint,
    <C as embedded_time::Clock>::T: From<<D as FixedPoint>::T>,
{
    pub fn new() -> Self {
        Self {
            subscriptions: Vec::new(),
        }
    }

    /// Add a subscription
    pub fn subscribe(
        &mut self,
        subscription: crate::Subscription<D>,
    ) -> Result<(), SubscriptionError> {
        if self.subscriptions.iter().any(|s| s.sub == subscription) {
            return Err(SubscriptionError::SubscriptionExists);
        }

        self.subscriptions.push(Subscription::new(subscription));
        Ok(())
    }

    /// Modify subscription in place, creating a new one if not found.
    pub fn edit_subscription(
        &mut self,
        subscription: crate::Subscription<D>,
    ) -> Result<(), SubscriptionError> {
        match self
            .subscriptions
            .iter()
            .position(|s| s.sub == subscription)
        {
            Some(pos) => {
                self.subscriptions[pos] = Subscription::new(subscription);
                Ok(())
            }
            None => Err(SubscriptionError::SubscriptionDoesNotExist),
        }
    }

    /// Removes a subscription from the list.
    pub fn unsubscribe(
        &mut self,
        subscription: crate::Subscription<D>,
    ) -> Result<(), SubscriptionError> {
        match self
            .subscriptions
            .iter()
            .position(|x| x.sub == subscription)
        {
            Some(pos) => {
                self.subscriptions.remove(pos);
                Ok(())
            }
            None => Err(SubscriptionError::SubscriptionDoesNotExist),
        }
    }
}

impl<T, D, C> SessionManager<C> for HeapSessionManager<T, D, C>
where
    T: crate::transport::SessionMetadata<C>,
    C: Clock,
    D: embedded_time::duration::Duration + FixedPoint,
    <C as embedded_time::Clock>::T: From<<D as FixedPoint>::T>,
{
    fn ingest(&mut self, frame: InternalRxFrame<C>) -> Result<Option<Transfer<C>>, SessionError> {
        match self
            .subscriptions
            .iter_mut()
            .find(|sub| Self::matches_sub(&sub.sub, &frame))
        {
            Some(subscription) => subscription.update(frame),
            None => Ok(None),
        }
    }

    fn update_sessions(&mut self, timestamp: Timestamp<C>) {
        for sub in &mut self.subscriptions {
            let extent = sub.sub.extent;
            for session in sub.sessions.values_mut() {
                if timestamp_expired(sub.sub.timeout, timestamp, session.timestamp) {
                    let transfer_id = session.transfer_id;
                    *session = Session::new(transfer_id, Some(extent));
                }
            }
        }
    }
}