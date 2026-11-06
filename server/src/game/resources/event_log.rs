//! The tick's events, sent with its snapshot.

use crate::pb;
use bevy_ecs::prelude::*;

#[derive(Resource, Default)]
pub struct EventLog(pub Vec<pb::GameEvent>);

impl EventLog {
    pub fn push(&mut self, event: pb::game_event::Event) {
        self.0.push(pb::GameEvent { event: Some(event) });
    }
}
