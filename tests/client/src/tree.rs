//! The behaviour tree every load-test bot runs (bonsai-bt):
//!
//! ```text
//! Sequence
//! ├─ AlwaysSucceed(Race)                    whichever ends first
//! │  ├─ AwaitShutdown                       the run's duration is up, or Ctrl+C
//! │  └─ Sequence
//! │     ├─ Hello                            name "bot_<random>"
//! │     ├─ Select
//! │     │  ├─ Sequence [IsLeader, CreateRoom]   one bot in ten makes the room
//! │     │  └─ JoinRoom                          the rest find it and join
//! │     └─ While(WaitForever)               match after match
//! │        ├─ Select
//! │        │  ├─ Sequence [IsLeader, StartMatch]  start once the group is in and ready
//! │        │  └─ ReadyUp
//! │        ├─ Fight                         until MatchEnd
//! │        └─ AwaitLobby                    until the results are over
//! └─ Leave
//! ```

use crate::agent::{Act, Agent};
use bonsai_bt::{
    Action, ActionArgs, AlwaysSucceed, Behavior, Event, Race, Select, Sequence, Status, UpdateArgs,
    WaitForever, While, BT,
};

pub fn behavior() -> Behavior<Act> {
    let lead_or = |lead: Act, follow: Act| {
        Select(vec![
            Sequence(vec![Action(Act::IsLeader), Action(lead)]),
            Action(follow),
        ])
    };
    let play = Sequence(vec![
        Action(Act::Hello),
        lead_or(Act::CreateRoom, Act::JoinRoom),
        While(
            Box::new(WaitForever),
            vec![
                lead_or(Act::StartMatch, Act::ReadyUp),
                Action(Act::Fight),
                Action(Act::AwaitLobby),
            ],
        ),
    ]);
    Sequence(vec![
        AlwaysSucceed(Box::new(Race(vec![Action(Act::AwaitShutdown), play]))),
        Action(Act::Leave),
    ])
}

/// A bot's tree with its blackboard.
pub struct Mind {
    bt: BT<Act, Agent>,
}

impl Mind {
    pub fn new(agent: Agent) -> Self {
        Self {
            bt: BT::new(behavior(), agent),
        }
    }

    pub fn agent(&mut self) -> &mut Agent {
        self.bt.blackboard_mut()
    }

    /// Runs the tree `dt` seconds after the last tick. True once it has finished: the bot has
    /// left and can hang up.
    pub fn tick(&mut self, dt: f64) -> bool {
        self.bt.blackboard_mut().advance(dt);
        let event: Event = UpdateArgs { dt }.into();
        let result = self.bt.tick(
            &event,
            &mut |args: ActionArgs<Event, Act>, agent: &mut Agent| {
                let status = agent.act(*args.action, args.dt);
                // A leaf that finished used none of the tick's time; a running one used all of it.
                let left = if status == Status::Running {
                    0.0
                } else {
                    args.dt
                };
                (status, left)
            },
        );
        result.is_none_or(|(status, _)| status != Status::Running)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::RoomSettings;
    use crate::pb::{self, client_message::Payload as Out, server_message::Payload as In};
    use crate::plan::{place, room_name};
    use crate::rng::Rng;

    const DT: f64 = 1.0 / 30.0;
    const SETTINGS: RoomSettings = RoomSettings {
        run: 1,
        mode: pb::GameMode::Normal,
        map: pb::MapId::SupplyYard,
        match_type: pb::MatchType::TeamDeathmatch,
    };

    fn mind(index: usize, count: usize) -> Mind {
        Mind::new(Agent::new(
            "bot_1".into(),
            place(index, count),
            SETTINGS,
            Rng::new(2),
        ))
    }

    fn hear(mind: &mut Mind, payload: In) {
        mind.agent().absorb(pb::ServerMessage {
            payload: Some(payload),
        });
    }

    /// Ticks the tree and returns what it sent.
    fn step(mind: &mut Mind) -> Vec<Out> {
        // Past any resend wait, so every request the tree wants shows at once.
        let finished = mind.tick(3.0);
        assert!(!finished, "the tree ended early");
        std::mem::take(&mut mind.agent().outbox)
    }

    fn waiting(players: &[(u32, bool)]) -> In {
        In::RoomState(pb::RoomState {
            room: Some(pb::RoomInfo {
                phase: pb::RoomPhase::Waiting as i32,
                ..Default::default()
            }),
            players: players
                .iter()
                .map(|&(id, ready)| pb::RoomPlayer {
                    id,
                    ready,
                    ..Default::default()
                })
                .collect(),
            bots: Vec::new(),
        })
    }

    fn match_start() -> In {
        In::MatchStart(pb::MatchStart {
            config: Some(pb::GameConfig {
                player_height: 1.8,
                eye_height: 1.42,
                ..Default::default()
            }),
            map: Some(pb::MapData {
                half_x: 5.0,
                half_z: 5.0,
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    #[test]
    fn a_leader_says_hello_makes_a_room_starts_plays_again_and_leaves_on_shutdown() {
        let mut leader = mind(0, 2);
        assert!(matches!(step(&mut leader)[..], [Out::Hello(_)]));
        hear(
            &mut leader,
            In::Welcome(pb::Welcome {
                player_id: 1,
                name: String::new(),
            }),
        );
        assert!(matches!(step(&mut leader)[..], [Out::CreateRoom(_)]));
        hear(&mut leader, waiting(&[(1, false)]));
        let sent = step(&mut leader);
        assert!(
            !sent.iter().any(|m| matches!(m, Out::StartGame(_))),
            "alone in the room"
        );
        hear(&mut leader, waiting(&[(1, false), (2, true)]));
        assert!(step(&mut leader)
            .iter()
            .any(|m| matches!(m, Out::StartGame(_))));

        // A match, the results, and the next start.
        hear(&mut leader, match_start());
        step(&mut leader);
        hear(&mut leader, In::MatchEnd(pb::MatchEnd::default()));
        step(&mut leader);
        hear(&mut leader, waiting(&[(1, false), (2, true)]));
        step(&mut leader);
        assert!(
            step(&mut leader)
                .iter()
                .any(|m| matches!(m, Out::StartGame(_))),
            "again"
        );

        leader.agent().shutdown = true;
        assert!(leader.tick(DT), "the tree ends once the bot has left");
        assert!(matches!(leader.agent().outbox[..], [Out::LeaveRoom(_)]));
    }

    #[test]
    fn a_member_joins_the_listed_room_readies_and_fights() {
        let mut member = mind(1, 2);
        step(&mut member);
        hear(
            &mut member,
            In::Welcome(pb::Welcome {
                player_id: 2,
                name: String::new(),
            }),
        );
        assert!(step(&mut member).is_empty(), "no room listed yet");
        hear(
            &mut member,
            In::RoomList(pb::RoomList {
                rooms: vec![pb::RoomInfo {
                    id: 8,
                    name: room_name(SETTINGS.run, 0),
                    ..Default::default()
                }],
            }),
        );
        assert!(matches!(
            step(&mut member)[..],
            [Out::JoinRoom(pb::JoinRoom { room_id: 8 })]
        ));
        hear(&mut member, waiting(&[(1, false), (2, false)]));
        assert!(step(&mut member)
            .iter()
            .any(|m| matches!(m, Out::SetReady(_))));
        hear(&mut member, match_start());
        hear(
            &mut member,
            In::Snapshot(pb::Snapshot {
                players: vec![pb::PlayerState {
                    id: 2,
                    alive: true,
                    ..Default::default()
                }],
                ..Default::default()
            }),
        );
        step(&mut member);
        assert!(step(&mut member).iter().any(|m| matches!(m, Out::Input(_))));
    }

    #[test]
    fn a_bot_told_to_stop_before_it_has_a_room_just_hangs_up() {
        let mut bot = mind(1, 2);
        step(&mut bot);
        bot.agent().shutdown = true;
        assert!(bot.tick(DT));
        assert!(bot.agent().outbox.is_empty());
    }
}
