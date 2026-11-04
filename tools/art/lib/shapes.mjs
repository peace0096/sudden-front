// Flat-shaded solids built in code (the missile, the machine gun's box magazine), added to a
// glTF document as their own node, mesh and material. Every face has its own vertices so the
// facets read like the low-poly packs they sit beside.

function linear(hex) {
  const value = Number.parseInt(hex.slice(1), 16);
  return [16, 8, 0].map((shift) => (((value >> shift) & 255) / 255) ** 2.2);
}

/** Collects triangles; quad and fan corners are given counter-clockwise from outside. */
class Solid {
  positions = [];
  normals = [];
  indices = [];

  face(corners) {
    const [a, b, c] = corners;
    const u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    const v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    const n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    const length = Math.hypot(...n) || 1;
    const base = this.positions.length / 3;
    for (const corner of corners) {
      this.positions.push(...corner);
      this.normals.push(n[0] / length, n[1] / length, n[2] / length);
    }
    for (let i = 1; i + 1 < corners.length; i++) this.indices.push(base, base + i, base + i + 1);
  }
}

/** An axis-aligned box from min to max. */
export function box(min, max) {
  const s = new Solid();
  const [x0, y0, z0] = min;
  const [x1, y1, z1] = max;
  s.face([[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]]);
  s.face([[x0, y0, z1], [x0, y1, z1], [x0, y1, z0], [x0, y0, z0]]);
  s.face([[x0, y1, z1], [x1, y1, z1], [x1, y1, z0], [x0, y1, z0]]);
  s.face([[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]]);
  s.face([[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]]);
  s.face([[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]]);
  return s;
}

const ring = (radius, z, segments, i) => {
  const angle = (i / segments) * Math.PI * 2;
  return [Math.cos(angle) * radius, Math.sin(angle) * radius, z];
};

/**
 * A solid of revolution about the Z axis through the given [radius, z] profile, from the back
 * (lowest z) forward; a radius of 0 makes a point. Each end is closed unless caps says otherwise,
 * for an end hidden inside another part.
 */
export function lathe(profile, segments, caps = { back: true, front: true }) {
  const s = new Solid();
  for (let p = 0; p + 1 < profile.length; p++) {
    const [r0, z0] = profile[p];
    const [r1, z1] = profile[p + 1];
    for (let i = 0; i < segments; i++) {
      const a0 = ring(r0, z0, segments, i);
      const a1 = ring(r0, z0, segments, i + 1);
      const b0 = ring(r1, z1, segments, i);
      const b1 = ring(r1, z1, segments, i + 1);
      if (r0 === 0) s.face([a0, b1, b0]);
      else if (r1 === 0) s.face([a0, a1, b0]);
      else s.face([a0, a1, b1, b0]);
    }
  }
  const [rFirst, zFirst] = profile[0];
  const [rLast, zLast] = profile.at(-1);
  if (rFirst > 0 && caps.back) s.face(Array.from({ length: segments }, (_, i) => ring(rFirst, zFirst, segments, segments - i)));
  if (rLast > 0 && caps.front) s.face(Array.from({ length: segments }, (_, i) => ring(rLast, zLast, segments, i)));
  return s;
}

/** Joins solids into one. */
export function merge(...solids) {
  const out = new Solid();
  for (const solid of solids) {
    const base = out.positions.length / 3;
    out.positions.push(...solid.positions);
    out.normals.push(...solid.normals);
    out.indices.push(...solid.indices.map((index) => index + base));
  }
  return out;
}

/** Adds a solid to the document's first scene as a node with its own mesh and a flat-coloured material. */
export function addSolid(doc, solid, { name, color, parent }) {
  const root = doc.getRoot();
  const buffer = root.listBuffers()[0] ?? doc.createBuffer();
  const accessor = (type, array) => doc.createAccessor().setType(type).setArray(array).setBuffer(buffer);
  const material = doc
    .createMaterial(name)
    .setBaseColorFactor([...linear(color), 1])
    .setMetallicFactor(0)
    .setRoughnessFactor(1);
  const primitive = doc
    .createPrimitive()
    .setAttribute("POSITION", accessor("VEC3", new Float32Array(solid.positions)))
    .setAttribute("NORMAL", accessor("VEC3", new Float32Array(solid.normals)))
    .setIndices(accessor("SCALAR", new Uint16Array(solid.indices)))
    .setMaterial(material);
  const node = doc.createNode(name).setMesh(doc.createMesh(name).addPrimitive(primitive));
  (parent ?? root.getDefaultScene() ?? root.listScenes()[0] ?? doc.createScene("scene")).addChild(node);
  return node;
}

export { linear as hexToLinear };
