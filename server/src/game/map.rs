//! The arenas. Each is authored for one team's half and point-mirrored, except
//! for self-symmetric centre pieces, so both teams get an identical layout.

use crate::pb::{BoxKind, MapBox, MapData, MapId, MapProp, PropKind};
use std::f32::consts::{FRAC_PI_2, PI};

pub struct MapDef {
    pub data: MapData,
    /// Red's spawn points; blue's are the mirror image.
    pub spawns: Vec<(f32, f32)>,
    /// Free-for-all spawn points, in world coordinates.
    pub ffa_spawns: Vec<(f32, f32)>,
    /// Places bots head to when they have no contact.
    pub lanes: Vec<(f32, f32)>,
}

pub fn build_map(id: MapId) -> MapDef {
    match id {
        MapId::Warehouse => warehouse(),
        _ => supply_yard(),
    }
}

/// Free-for-all spawn points: both teams' spawns, plus `open` points elsewhere picked by hand,
/// each with its mirror image so neither half is favoured. An open point needs headroom, which
/// rules out doorways and the warehouse's vent tunnel.
fn ffa_spawns(spawns: &[(f32, f32)], open: &[(f32, f32)]) -> Vec<(f32, f32)> {
    spawns
        .iter()
        .chain(open)
        .flat_map(|&(x, z)| [(x, z), (-x, -z)])
        .collect()
}

fn assemble(
    id: MapId,
    name: &str,
    half_x: f32,
    half_z: f32,
    half: Vec<MapBox>,
    centre: Vec<MapBox>,
    props: Vec<MapProp>,
) -> MapData {
    let mut boxes: Vec<MapBox> = half.iter().map(mirror_box).collect();
    boxes.extend(half);
    boxes.extend(centre);
    let mut all_props: Vec<MapProp> = props.iter().map(mirror_prop).collect();
    all_props.extend(props);
    // A leaning container collides as slices that follow it.
    for p in all_props.iter().filter(|p| p.kind == PropKind::TiltedContainer as i32) {
        boxes.extend(lean_slices(p));
    }
    MapData {
        name: name.into(),
        id: id as i32,
        half_x,
        half_z,
        boxes,
        props: all_props,
    }
}

fn bx(kind: BoxKind, x0: f32, x1: f32, y0: f32, y1: f32, z0: f32, z1: f32) -> MapBox {
    MapBox {
        kind: kind as i32,
        min_x: x0,
        max_x: x1,
        min_y: y0,
        max_y: y1,
        min_z: z0,
        max_z: z1,
    }
}

fn prop(kind: PropKind, x: f32, y: f32, z: f32, rotation: f32, width: f32, height: f32) -> MapProp {
    MapProp {
        kind: kind as i32,
        x,
        y,
        z,
        rotation,
        width,
        height,
        tilt: 0.0,
    }
}

/// A container's height, which a leaning one keeps: the client's world.ts draws it this tall
/// (TILTED_HEIGHT).
const CONTAINER_H: f32 = 2.6;
/// How far the leaning container tips its +X end up, radians.
const LEAN: f32 = 0.34;
/// The longest a leaning container's collision slice is along it: as tall as the container is
/// anywhere across it, a slice stands at most this × tan(LEAN) (0.18 m) above the slope.
const LEAN_SLICE: f32 = 0.5;

/// The boxes a leaning container (a TILTED_CONTAINER prop: `width` its length, `height` its depth)
/// collides as: slices across its length, each from its underside, or the floor, to its top across
/// the slice. A shot over its low end then flies on, and a slice is a box top to stand on. Exact
/// for a prop turned by a multiple of a quarter turn, as both of the warehouse's are.
fn lean_slices(p: &MapProp) -> Vec<MapBox> {
    let (half_length, half_height, half_depth) = (p.width / 2.0, CONTAINER_H / 2.0, p.height / 2.0);
    let (sin, cos) = p.tilt.sin_cos();
    // The container's side tipped over, as (along, up) corners in order round it.
    let corners: Vec<(f32, f32)> = [(-half_length, -half_height), (half_length, -half_height), (half_length, half_height), (-half_length, half_height)]
        .into_iter()
        .map(|(u, v)| (u * cos - v * sin, u * sin + v * cos))
        .collect();
    let start = corners.iter().map(|c| c.0).fold(f32::MAX, f32::min);
    let end = corners.iter().map(|c| c.0).fold(f32::MIN, f32::max);
    let count = ((end - start) / LEAN_SLICE).ceil().max(1.0) as usize;
    let length = (end - start) / count as f32;
    // Turned about +Y as three.js turns the model: x' = x cos + z sin, z' = −x sin + z cos.
    let (yaw_sin, yaw_cos) = p.rotation.sin_cos();
    let world = |u: f32, w: f32| (p.x + u * yaw_cos + w * yaw_sin, p.z - u * yaw_sin + w * yaw_cos);
    (0..count)
        .filter_map(|i| {
            let (u0, u1) = (start + length * i as f32, start + length * (i + 1) as f32);
            // The heights the side reaches across the slice: its corners within it, and where its
            // edges cross the slice's ends.
            let mut heights: Vec<f32> = corners.iter().filter(|c| (u0..=u1).contains(&c.0)).map(|c| c.1).collect();
            for k in 0..corners.len() {
                let ((ua, va), (ub, vb)) = (corners[k], corners[(k + 1) % corners.len()]);
                for cut in [u0, u1] {
                    if ua != ub && (ua - cut) * (ub - cut) <= 0.0 {
                        heights.push(va + (vb - va) * (cut - ua) / (ub - ua));
                    }
                }
            }
            let low = heights.iter().copied().reduce(f32::min)?;
            let high = heights.iter().copied().reduce(f32::max)?;
            let ends = [world(u0, -half_depth), world(u0, half_depth), world(u1, -half_depth), world(u1, half_depth)];
            let (x0, x1) = (ends.iter().map(|e| e.0).fold(f32::MAX, f32::min), ends.iter().map(|e| e.0).fold(f32::MIN, f32::max));
            let (z0, z1) = (ends.iter().map(|e| e.1).fold(f32::MAX, f32::min), ends.iter().map(|e| e.1).fold(f32::MIN, f32::max));
            Some(bx(BoxKind::Invisible, x0, x1, (p.y + low).max(0.0), p.y + high, z0, z1))
        })
        .collect()
}

fn mirror_box(b: &MapBox) -> MapBox {
    MapBox {
        kind: b.kind,
        min_x: -b.max_x,
        max_x: -b.min_x,
        min_y: b.min_y,
        max_y: b.max_y,
        min_z: -b.max_z,
        max_z: -b.min_z,
    }
}

fn mirror_prop(p: &MapProp) -> MapProp {
    MapProp {
        x: -p.x,
        z: -p.z,
        rotation: p.rotation + PI,
        ..p.clone()
    }
}

fn yard_boxes() -> Vec<MapBox> {
    use BoxKind::*;
    let crate_h = 1.2;
    vec![
        // Perimeter.
        bx(Brick, -51.0, 51.0, 0.0, 8.0, -33.0, -32.0),
        bx(Brick, -51.0, -50.0, 0.0, 8.0, -33.0, 33.0),
        // Solid warehouse block and its loading dock facing the centre lane.
        bx(Brick, -36.0, -8.0, 0.0, 7.0, -17.0, -7.0),
        bx(Concrete, -33.0, -11.0, 0.0, 1.0, -7.0, -5.4),
        // Walk-through warehouse: north wall with door.
        bx(Brick, -34.0, -26.0, 0.0, 6.0, 7.0, 7.5),
        bx(Brick, -22.0, -14.0, 0.0, 6.0, 7.0, 7.5),
        bx(Brick, -26.0, -22.0, 2.8, 6.0, 7.0, 7.5),
        // South wall with door.
        bx(Brick, -34.0, -20.0, 0.0, 6.0, 18.5, 19.0),
        bx(Brick, -16.0, -14.0, 0.0, 6.0, 18.5, 19.0),
        bx(Brick, -20.0, -16.0, 2.8, 6.0, 18.5, 19.0),
        // West wall with door.
        bx(Brick, -34.0, -33.5, 0.0, 6.0, 7.5, 11.0),
        bx(Brick, -34.0, -33.5, 0.0, 6.0, 15.0, 18.5),
        bx(Brick, -34.0, -33.5, 2.8, 6.0, 11.0, 15.0),
        // East wall with door.
        bx(Brick, -14.5, -14.0, 0.0, 6.0, 7.5, 11.0),
        bx(Brick, -14.5, -14.0, 0.0, 6.0, 15.0, 18.5),
        bx(Brick, -14.5, -14.0, 2.8, 6.0, 11.0, 15.0),
        // Roof.
        bx(Roof, -34.5, -13.5, 6.0, 6.4, 6.5, 19.5),
        // Containers inside the warehouse.
        bx(ContainerGray, -31.0, -28.6, 0.0, 2.6, 8.5, 14.5),
        bx(ContainerBlue, -18.6, -16.2, 0.0, 2.6, 9.0, 15.0),
        bx(Crate, -25.0, -23.8, 0.0, crate_h, 13.0, 14.2),
        bx(Crate, -26.2, -25.0, 0.0, crate_h, 13.0, 14.2),
        bx(Crate, -25.6, -24.4, crate_h, crate_h * 2.0, 13.0, 14.2),
        // South yard.
        bx(ContainerRust, -30.0, -24.0, 0.0, 2.6, 23.0, 25.4),
        bx(ContainerBlue, -30.0, -24.0, 2.6, 5.2, 23.0, 25.4),
        bx(ContainerGray, -12.0, -6.0, 0.0, 2.6, 24.0, 26.4),
        bx(Crate, -40.0, -38.8, 0.0, crate_h, 22.0, 23.2),
        bx(Crate, -38.8, -37.6, 0.0, crate_h, 22.0, 23.2),
        bx(Barrel, -19.0, -18.1, 0.0, 1.1, 25.0, 25.9),
        bx(Barrel, -18.0, -17.1, 0.0, 1.1, 25.6, 26.5),
        // North lane.
        bx(ContainerGray, -28.0, -22.0, 0.0, 2.6, -27.0, -24.6),
        bx(Crate, -16.0, -14.8, 0.0, crate_h, -22.0, -20.8),
        bx(Crate, -14.8, -13.6, 0.0, crate_h, -22.0, -20.8),
        bx(Crate, -15.4, -14.2, crate_h, crate_h * 2.0, -22.0, -20.8),
        bx(Barrel, -40.0, -39.1, 0.0, 1.1, -25.0, -24.1),
        // Spawn cover.
        bx(Crate, -40.0, -38.8, 0.0, crate_h, -4.0, -2.8),
        bx(Crate, -40.0, -38.8, 0.0, crate_h, 3.0, 4.2),
        // Mid-north crate pile (mirrors into mid-south).
        bx(Crate, 2.0, 3.2, 0.0, crate_h, -12.0, -10.8),
        bx(Crate, 3.2, 4.4, 0.0, crate_h, -12.0, -10.8),
        // Cover breaking up the centre lane from the spawn to the middle: a crate pile, a concrete
        // barrier low enough to jump onto, and two barrels.
        bx(Crate, -31.2, -30.0, 0.0, crate_h, 1.6, 2.8),
        bx(Crate, -30.0, -28.8, 0.0, crate_h, 1.6, 2.8),
        bx(Crate, -30.6, -29.4, crate_h, crate_h * 2.0, 1.6, 2.8),
        bx(Concrete, -21.5, -18.5, 0.0, 1.1, -2.0, -1.4),
        bx(Barrel, -13.4, -12.5, 0.0, 1.1, 1.6, 2.5),
        bx(Barrel, -12.3, -11.4, 0.0, 1.1, 2.0, 2.9),
        // A container between the solid warehouse's end and the middle.
        bx(ContainerBlue, -5.4, -3.0, 0.0, 2.6, -16.5, -10.5),
        // The open middle of the north lane: a barrier and a crate pile.
        bx(Concrete, -6.0, -3.0, 0.0, 1.1, -26.3, -25.7),
        bx(Crate, -12.6, -11.4, 0.0, crate_h, -29.4, -28.2),
        bx(Crate, -11.4, -10.2, 0.0, crate_h, -29.4, -28.2),
        bx(Crate, -12.0, -10.8, crate_h, crate_h * 2.0, -29.4, -28.2),
        // Barrels between the spawn and the walk-through warehouse.
        bx(Barrel, -39.0, -38.1, 0.0, 1.1, 11.2, 12.1),
        bx(Barrel, -38.0, -37.1, 0.0, 1.1, 11.6, 12.5),
    ]
}

fn yard_props() -> Vec<MapProp> {
    use PropKind::*;
    let mut props = Vec::new();
    // Roller shutters on the solid warehouse, facing the centre lane (+z).
    for x in [-30.0, -25.0, -20.0, -15.0] {
        props.push(prop(Shutter, x, 2.45, -6.97, 0.0, 4.0, 2.9));
    }
    props.push(prop(Billboard, -22.5, 5.0, -6.96, 0.0, 9.0, 1.8));
    // Windows on the back of the solid warehouse, facing the north lane (-z).
    for x in [-32.0, -26.0, -20.0, -14.0] {
        props.push(prop(Window, x, 3.8, -17.03, PI, 3.0, 1.8));
    }
    // Windows on the end of the solid warehouse facing the middle (+x).
    props.push(prop(Window, -7.97, 3.8, -12.0, FRAC_PI_2, 3.0, 1.8));
    // Windows on the walk-through warehouse.
    for x in [-31.0, -18.0] {
        props.push(prop(Window, x, 3.6, 6.97, PI, 2.6, 1.5));
    }
    // Lamps along the lanes.
    props.push(prop(Lamp, -36.0, 0.0, -4.4, 0.0, 0.0, 0.0));
    props.push(prop(Lamp, -10.0, 0.0, -4.4, 0.0, 0.0, 0.0));
    props.push(prop(Lamp, -30.0, 0.0, -30.8, PI, 0.0, 0.0));
    props.push(prop(Lamp, -10.0, 0.0, 30.8, 0.0, 0.0, 0.0));
    // Painted line along the loading dock.
    props.push(prop(FloorLine, -22.0, 0.015, -4.9, 0.0, 22.0, 0.16));
    // Warehouse lights.
    props.push(prop(CeilingLight, -24.0, 5.85, 10.5, 0.0, 0.0, 0.0));
    props.push(prop(CeilingLight, -24.0, 5.85, 16.0, 0.0, 0.0, 0.0));
    props
}

fn supply_yard() -> MapDef {
    let centre = vec![bx(BoxKind::ContainerRust, -1.2, 1.2, 0.0, 2.6, -3.0, 3.0)];
    let spawns = vec![
        (-46.0, -24.0),
        (-46.0, -15.0),
        (-46.0, -8.0),
        (-46.0, 0.0),
        (-46.0, 8.0),
        (-46.0, 15.0),
        (-46.0, 24.0),
    ];
    MapDef {
        data: assemble(MapId::SupplyYard, "SUPPLY YARD", 50.0, 32.0, yard_boxes(), centre, yard_props()),
        ffa_spawns: ffa_spawns(
            &spawns,
            &[
                // The middle of the north lane, between the warehouses.
                (0.0, -20.0),
                // The centre lane, and inside the walk-through warehouse.
                (-24.0, 0.0),
                (-24.0, 12.0),
                // The north lane and the south yard.
                (-18.0, -28.0),
                (-18.0, 28.0),
            ],
        ),
        spawns,
        lanes: vec![
            (0.0, -13.0),
            (0.0, 13.0),
            (-24.0, 0.0),
            (24.0, 0.0),
            (-24.0, 12.0),
            (24.0, -12.0),
            (-18.0, 28.0),
            (18.0, -28.0),
            (-18.0, -28.0),
            (18.0, 28.0),
            (-42.0, 0.0),
            (42.0, 0.0),
        ],
    }
}

// Warehouse: a small indoor container hall. Stacks block long sight lines, each
// base opens through a left and a right door, and a low vent tunnel along the
// wall gives a third way out.
const WH_HALF_X: f32 = 30.0;
const WH_HALF_Z: f32 = 20.0;
const WH_CEILING: f32 = 9.0;
const CRATE_H: f32 = 1.2;

/// The ways up onto the warehouse's single containers (2.6 m), one each in a half: `(x, z)` is the
/// middle of a stack of two crates (2.4 m) against the container's side, and `(dx, dz)` the way
/// from it to a single crate in front. A jump rises about 1.34 m: from the floor onto the crate,
/// from the crate onto the stack, and from the stack onto the container.
pub const WAREHOUSE_CLIMBS: [(f32, f32, f32, f32); 6] = [
    // The base's container, from the north.
    (-28.3, 12.9, 0.0, -1.0),
    // The hall's containers.
    (-15.6, -3.0, -1.0, 0.0),
    (-7.6, -7.8, -1.0, 0.0),
    (-4.4, 9.0, 0.0, 1.0),
    (-3.0, -15.0, 1.0, 0.0),
    (-4.4, 18.2, 1.0, 0.0),
];

/// The three crates of one of `WAREHOUSE_CLIMBS`.
fn climb((x, z, dx, dz): (f32, f32, f32, f32)) -> [MapBox; 3] {
    let r = CRATE_H / 2.0;
    let at = |x: f32, z: f32, y0: f32| bx(BoxKind::Crate, x - r, x + r, y0, y0 + CRATE_H, z - r, z + r);
    [at(x, z, 0.0), at(x, z, CRATE_H), at(x + dx * CRATE_H, z + dz * CRATE_H, 0.0)]
}

fn warehouse_boxes() -> Vec<MapBox> {
    use BoxKind::*;
    let crate_h = 1.2;
    let c = 2.6; // Container height.
    let door = 3.0;
    let wall = 4.6; // Base partition height.
    let mut boxes = vec![
        // Shell.
        bx(Brick, -31.0, 31.0, 0.0, WH_CEILING, -21.0, -20.0),
        bx(Brick, -31.0, -30.0, 0.0, WH_CEILING, -21.0, 21.0),
        // Base partition: gap for the vent at z -20..-17.6, left door at z -13..-9,
        // right door at z 5..9.
        bx(Concrete, -21.5, -21.0, 0.0, wall, -17.6, -13.0),
        bx(Concrete, -21.5, -21.0, door, wall, -13.0, -9.0),
        bx(Concrete, -21.5, -21.0, 0.0, wall, -9.0, 5.0),
        bx(Concrete, -21.5, -21.0, door, wall, 5.0, 9.0),
        bx(Concrete, -21.5, -21.0, 0.0, wall, 9.0, 20.0),
        // Vent tunnel: wall, lid and a mouth into the hall.
        bx(Roof, -21.5, -9.0, 2.3, 2.6, -20.0, -17.1),
        bx(Concrete, -21.0, -9.0, 0.0, 2.3, -17.6, -17.1),
        // Base cover.
        bx(ContainerGray, -29.5, -27.1, 0.0, c, 13.5, 19.5),
        bx(Crate, -24.6, -23.4, 0.0, crate_h, -3.0, -1.8),
        bx(Crate, -24.6, -23.4, 0.0, crate_h, -1.8, -0.6),
        bx(Crate, -24.6, -23.4, crate_h, crate_h * 2.0, -2.4, -1.2),
        // Hall: containers along and across the aisles.
        bx(ContainerPale, -17.0, -11.0, 0.0, c, -15.0, -12.6),
        bx(ContainerBlue, -17.0, -11.0, c, c * 2.0, -15.0, -12.6),
        bx(ContainerPale, -15.0, -12.6, 0.0, c, -6.0, 0.0),
        bx(ContainerPale, -7.0, -1.0, 0.0, c, -9.0, -6.6),
        bx(ContainerGray, -8.0, -2.0, 0.0, c, 6.0, 8.4),
        bx(ContainerPale, -15.0, -9.0, 0.0, c, 8.0, 10.4),
        bx(ContainerPale, -15.0, -9.0, c, c * 2.0, 8.0, 10.4),
        bx(ContainerPale, -6.0, -3.6, 0.0, c, -19.0, -13.0),
        // What the leaning container (a prop, which collides as slices of itself) rests on.
        bx(Crate, -6.4, -5.2, 0.0, crate_h, 14.6, 15.8),
        bx(ContainerPale, -11.0, -5.0, 0.0, c, 17.0, 19.4),
        // Crates in the aisles.
        bx(Crate, -19.0, -17.8, 0.0, crate_h, 2.0, 3.2),
        bx(Crate, -19.0, -17.8, 0.0, crate_h, 3.2, 4.4),
        bx(Crate, -9.5, -8.3, 0.0, crate_h, -3.0, -1.8),
    ];
    boxes.extend(WAREHOUSE_CLIMBS.into_iter().flat_map(climb));
    boxes
}

fn warehouse_props() -> Vec<MapProp> {
    use PropKind::*;
    // A container tipped up onto a crate, its high end east.
    let mut props = vec![MapProp {
        tilt: LEAN,
        ..prop(TiltedContainer, -8.0, 1.7, 15.2, 0.0, 6.0, 2.4)
    }];
    // Roof trusses and lights over each half, louvered vents high on the end wall.
    for x in [-26.0, -18.0, -10.0, -2.0] {
        props.push(prop(Truss, x, WH_CEILING - 1.5, 0.0, 0.0, WH_HALF_Z * 2.0, 1.4));
    }
    for (x, z) in [(-22.0, -10.0), (-22.0, 10.0), (-8.0, -10.0), (-8.0, 10.0)] {
        props.push(prop(CeilingLight, x, WH_CEILING - 2.3, z, 0.0, 0.0, 0.0));
    }
    for z in [-13.0, -4.0, 5.0, 14.0] {
        props.push(prop(Louver, -29.97, 6.6, z, FRAC_PI_2, 5.0, 1.8));
    }
    for x in [-20.0, -8.0] {
        props.push(prop(Louver, x, 6.6, -19.97, 0.0, 5.0, 1.8));
    }
    // Hazard line at each base door.
    props.push(prop(FloorLine, -20.4, 0.015, -11.0, FRAC_PI_2, 4.0, 0.16));
    props.push(prop(FloorLine, -20.4, 0.015, 7.0, FRAC_PI_2, 4.0, 0.16));
    props
}

fn warehouse() -> MapDef {
    use BoxKind::*;
    let crate_h = 1.2;
    let centre = vec![
        bx(Roof, -WH_HALF_X - 1.0, WH_HALF_X + 1.0, WH_CEILING, WH_CEILING + 0.4, -WH_HALF_Z - 1.0, WH_HALF_Z + 1.0),
        bx(Crate, -1.2, 0.0, 0.0, crate_h, 0.0, 1.2),
        bx(Crate, 0.0, 1.2, 0.0, crate_h, -1.2, 0.0),
        bx(Crate, -0.6, 0.6, crate_h, crate_h * 2.0, -0.6, 0.6),
    ];
    let spawns = vec![
        (-26.0, -16.0),
        (-26.0, -11.0),
        (-26.0, -6.0),
        (-26.0, 0.0),
        (-26.0, 5.0),
        (-26.0, 10.0),
        (-25.0, 16.0),
    ];
    MapDef {
        data: assemble(MapId::Warehouse, "WAREHOUSE", WH_HALF_X, WH_HALF_Z, warehouse_boxes(), centre, warehouse_props()),
        // The hall: the bots' lanes but the two in the vent tunnel, and beside the tilted container.
        ffa_spawns: ffa_spawns(
            &spawns,
            &[(-5.0, 0.0), (-19.0, -10.0), (-19.0, 8.0), (-8.0, -12.0), (-10.0, 4.0), (-3.0, 16.0)],
        ),
        spawns,
        lanes: vec![
            (-5.0, 0.0),
            (5.0, 0.0),
            (-19.0, -10.0),
            (19.0, 10.0),
            (-19.0, 8.0),
            (19.0, -8.0),
            (-8.0, -12.0),
            (8.0, 12.0),
            (-10.0, 4.0),
            (10.0, -4.0),
            (-15.0, -18.5),
            (15.0, 18.5),
        ],
    }
}
