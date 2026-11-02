//! The arenas. Each is authored for one team's half and point-mirrored, except
//! for self-symmetric centre pieces, so both teams get an identical layout.

use crate::pb::{BoxKind, MapBox, MapData, MapId, MapProp, PropKind};
use std::f32::consts::{FRAC_PI_2, PI};

pub struct MapDef {
    pub data: MapData,
    /// Red's spawn points; blue's are the mirror image.
    pub spawns: Vec<(f32, f32)>,
    /// Places bots head to when they have no contact.
    pub lanes: Vec<(f32, f32)>,
}

pub fn build_map(id: MapId) -> MapDef {
    match id {
        MapId::Warehouse => warehouse(),
        _ => supply_yard(),
    }
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
    }
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
    MapDef {
        data: assemble(MapId::SupplyYard, "SUPPLY YARD", 50.0, 32.0, yard_boxes(), centre, yard_props()),
        spawns: vec![
            (-46.0, -24.0),
            (-46.0, -15.0),
            (-46.0, -8.0),
            (-46.0, 0.0),
            (-46.0, 8.0),
            (-46.0, 15.0),
            (-46.0, 24.0),
        ],
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

fn warehouse_boxes() -> Vec<MapBox> {
    use BoxKind::*;
    let crate_h = 1.2;
    let c = 2.6; // Container height.
    let door = 3.0;
    let wall = 4.6; // Base partition height.
    vec![
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
        // Footprint of the container leaning on the stack.
        bx(Invisible, -11.0, -5.0, 0.0, 4.2, 13.5, 17.0),
        // What it leans on.
        bx(Crate, -6.4, -5.2, 0.0, crate_h, 14.6, 15.8),
        bx(ContainerPale, -11.0, -5.0, 0.0, c, 17.0, 19.4),
        // Crates in the aisles.
        bx(Crate, -19.0, -17.8, 0.0, crate_h, 2.0, 3.2),
        bx(Crate, -19.0, -17.8, 0.0, crate_h, 3.2, 4.4),
        bx(Crate, -9.5, -8.3, 0.0, crate_h, -3.0, -1.8),
    ]
}

fn warehouse_props() -> Vec<MapProp> {
    use PropKind::*;
    let mut props = vec![prop(TiltedContainer, -8.0, 1.7, 15.2, 0.0, 6.0, 2.4)];
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
    MapDef {
        data: assemble(MapId::Warehouse, "WAREHOUSE", WH_HALF_X, WH_HALF_Z, warehouse_boxes(), centre, warehouse_props()),
        spawns: vec![
            (-26.0, -16.0),
            (-26.0, -11.0),
            (-26.0, -6.0),
            (-26.0, 0.0),
            (-26.0, 5.0),
            (-26.0, 10.0),
            (-25.0, 16.0),
        ],
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
