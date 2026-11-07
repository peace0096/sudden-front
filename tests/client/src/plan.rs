//! How a run's bots split into rooms: every ten bots in start order share one room, and the first of
//! them, the leader, makes it and starts its matches.

/// The most players a room seats (the server's `hub::MAX_PLAYERS`).
pub const ROOM_SIZE: usize = 10;

/// One bot's place in the run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// Which room, from 0.
    pub group: usize,
    /// Makes the room and starts its matches.
    pub leader: bool,
    /// How many bots share the room, the leader with them.
    pub size: usize,
}

/// The place of bot `index` (from 0) in a run of `count` bots.
pub fn place(index: usize, count: usize) -> Place {
    let group = index / ROOM_SIZE;
    let first = group.saturating_mul(ROOM_SIZE);
    Place {
        group,
        leader: index == first,
        size: count.saturating_sub(first).min(ROOM_SIZE),
    }
}

/// The room a group plays in, named so a run's bots find it in the room list and never join
/// another run's room. The server keeps room names to 24 characters.
pub fn room_name(run: u32, group: usize) -> String {
    format!("loadtest-{:04x}-{group}", run & 0xffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ten_bots_share_a_room_led_by_the_first_of_them() {
        let places: Vec<Place> = (0..25).map(|i| place(i, 25)).collect();
        assert_eq!(places.iter().filter(|p| p.leader).count(), 3);
        for (index, place) in places.iter().enumerate() {
            assert_eq!(place.group, index / ROOM_SIZE);
            assert_eq!(place.leader, index % ROOM_SIZE == 0);
        }
    }

    #[test]
    fn the_last_room_holds_only_the_bots_left_over() {
        assert_eq!(place(0, 25).size, ROOM_SIZE);
        assert_eq!(place(19, 25).size, ROOM_SIZE);
        assert_eq!(place(20, 25).size, 5);
        assert_eq!(place(24, 25).size, 5);
        assert_eq!(place(0, 1).size, 1);
    }

    #[test]
    fn a_room_name_fits_the_servers_limit_and_differs_by_run_and_group() {
        let name = room_name(u32::MAX, 999);
        assert!(name.chars().count() <= 24, "{name}");
        assert_ne!(room_name(1, 0), room_name(2, 0));
        assert_ne!(room_name(1, 0), room_name(1, 1));
    }
}
