// Column-major 4×4 matrices and [x, y, z, w] quaternions, as glTF stores them.

export const identity = () => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];

export function multiply(a, b) {
  const out = new Array(16);
  for (let c = 0; c < 4; c++) {
    for (let r = 0; r < 4; r++) {
      out[c * 4 + r] = a[r] * b[c * 4] + a[4 + r] * b[c * 4 + 1] + a[8 + r] * b[c * 4 + 2] + a[12 + r] * b[c * 4 + 3];
    }
  }
  return out;
}

/** a × b × c × ...; the last matrix is applied first. */
export const chain = (...matrices) => matrices.reduce((acc, m) => multiply(acc, m));

export function invert(m) {
  const [a00, a01, a02, a03, a10, a11, a12, a13, a20, a21, a22, a23, a30, a31, a32, a33] = m;
  const b00 = a00 * a11 - a01 * a10;
  const b01 = a00 * a12 - a02 * a10;
  const b02 = a00 * a13 - a03 * a10;
  const b03 = a01 * a12 - a02 * a11;
  const b04 = a01 * a13 - a03 * a11;
  const b05 = a02 * a13 - a03 * a12;
  const b06 = a20 * a31 - a21 * a30;
  const b07 = a20 * a32 - a22 * a30;
  const b08 = a20 * a33 - a23 * a30;
  const b09 = a21 * a32 - a22 * a31;
  const b10 = a21 * a33 - a23 * a31;
  const b11 = a22 * a33 - a23 * a32;
  const det = b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;
  if (!det) throw new Error("matrix is not invertible");
  const d = 1 / det;
  return [
    (a11 * b11 - a12 * b10 + a13 * b09) * d,
    (a02 * b10 - a01 * b11 - a03 * b09) * d,
    (a31 * b05 - a32 * b04 + a33 * b03) * d,
    (a22 * b04 - a21 * b05 - a23 * b03) * d,
    (a12 * b08 - a10 * b11 - a13 * b07) * d,
    (a00 * b11 - a02 * b08 + a03 * b07) * d,
    (a32 * b02 - a30 * b05 - a33 * b01) * d,
    (a20 * b05 - a22 * b02 + a23 * b01) * d,
    (a10 * b10 - a11 * b08 + a13 * b06) * d,
    (a01 * b08 - a00 * b10 - a03 * b06) * d,
    (a30 * b04 - a31 * b02 + a33 * b00) * d,
    (a21 * b02 - a20 * b04 - a23 * b00) * d,
    (a11 * b07 - a10 * b09 - a12 * b06) * d,
    (a00 * b09 - a01 * b07 + a02 * b06) * d,
    (a31 * b01 - a30 * b03 - a32 * b00) * d,
    (a20 * b03 - a21 * b01 + a22 * b00) * d,
  ];
}

export const translation = ([x, y, z]) => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, x, y, z, 1];

export const scaling = ([x, y, z]) => [x, 0, 0, 0, 0, y, 0, 0, 0, 0, z, 0, 0, 0, 0, 1];

/** A rotation of `degrees` about the x, y or z axis. */
export function rotation(axis, degrees) {
  const r = (degrees * Math.PI) / 180;
  const c = Math.cos(r);
  const s = Math.sin(r);
  if (axis === "x") return [1, 0, 0, 0, 0, c, s, 0, 0, -s, c, 0, 0, 0, 0, 1];
  if (axis === "y") return [c, 0, -s, 0, 0, 1, 0, 0, s, 0, c, 0, 0, 0, 0, 1];
  return [c, s, 0, 0, -s, c, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
}

export function fromQuaternion([x, y, z, w]) {
  return [
    1 - 2 * (y * y + z * z), 2 * (x * y + z * w), 2 * (x * z - y * w), 0,
    2 * (x * y - z * w), 1 - 2 * (x * x + z * z), 2 * (y * z + x * w), 0,
    2 * (x * z + y * w), 2 * (y * z - x * w), 1 - 2 * (x * x + y * y), 0,
    0, 0, 0, 1,
  ];
}

/** Translation × rotation × scale, as a glTF node composes them. */
export function compose(t, q, s) {
  const m = fromQuaternion(q);
  for (let c = 0; c < 3; c++) for (let r = 0; r < 3; r++) m[c * 4 + r] *= s[c];
  m[12] = t[0];
  m[13] = t[1];
  m[14] = t[2];
  return m;
}

export const transformPoint = (m, [x, y, z]) => [
  m[0] * x + m[4] * y + m[8] * z + m[12],
  m[1] * x + m[5] * y + m[9] * z + m[13],
  m[2] * x + m[6] * y + m[10] * z + m[14],
];

export const transformVector = (m, [x, y, z]) => [m[0] * x + m[4] * y + m[8] * z, m[1] * x + m[5] * y + m[9] * z, m[2] * x + m[6] * y + m[10] * z];

/** a ⊗ b: the rotation b followed by a. */
export function quatMultiply([ax, ay, az, aw], [bx, by, bz, bw]) {
  return [
    aw * bx + ax * bw + ay * bz - az * by,
    aw * by - ax * bz + ay * bw + az * bx,
    aw * bz + ax * by - ay * bx + az * bw,
    aw * bw - ax * bx - ay * by - az * bz,
  ];
}

/** The rotation part of a matrix without scale, as a quaternion. */
export function quatFromMatrix(m) {
  const [m11, m21, m31, , m12, m22, m32, , m13, m23, m33] = m;
  const trace = m11 + m22 + m33;
  if (trace > 0) {
    const s = 0.5 / Math.sqrt(trace + 1);
    return [(m32 - m23) * s, (m13 - m31) * s, (m21 - m12) * s, 0.25 / s];
  }
  if (m11 > m22 && m11 > m33) {
    const s = 2 * Math.sqrt(1 + m11 - m22 - m33);
    return [0.25 * s, (m12 + m21) / s, (m13 + m31) / s, (m32 - m23) / s];
  }
  if (m22 > m33) {
    const s = 2 * Math.sqrt(1 + m22 - m11 - m33);
    return [(m12 + m21) / s, 0.25 * s, (m23 + m32) / s, (m13 - m31) / s];
  }
  const s = 2 * Math.sqrt(1 + m33 - m11 - m22);
  return [(m13 + m31) / s, (m23 + m32) / s, 0.25 * s, (m21 - m12) / s];
}

/** Euler angles in XYZ order, as three.js Euler.setFromRotationMatrix reads them. */
export function eulerXYZ(m) {
  const [m11, , , , m12, m22, m32, , m13, m23, m33] = m;
  const y = Math.asin(Math.min(1, Math.max(-1, m13)));
  if (Math.abs(m13) < 0.9999999) return [Math.atan2(-m23, m33), y, Math.atan2(-m12, m11)];
  return [Math.atan2(m32, m22), y, 0];
}

export const round = (value, places = 4) => Math.round(value * 10 ** places) / 10 ** places || 0;

// [x, y, z] vectors.
export const add = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
export const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
export const scale = (a, s) => [a[0] * s, a[1] * s, a[2] * s];
export const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
export const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
export const normalize = (a) => scale(a, 1 / Math.hypot(a[0], a[1], a[2]));

/** The determinant of a 3×3 matrix given as rows. */
const det3 = (m) =>
  m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);

/**
 * The circle that best fits points along an arc lying near one plane: the plane through their
 * centroid square to the turn the arc makes, then Kåsa's least squares in it. Returns its centre
 * and radius, and the plane's normal, the way the arc turns by the right-hand rule.
 */
export function fitCircle(points) {
  const centroid = scale(points.reduce(add), 1 / points.length);
  let turn = [0, 0, 0];
  for (let i = 0; i + 1 < points.length; i++) turn = add(turn, cross(sub(points[i], centroid), sub(points[i + 1], centroid)));
  // Points in a line make no turn and have no plane of their own.
  if (Math.hypot(turn[0], turn[1], turn[2]) < 1e-12) throw new Error("the points do not bend round a circle");
  const normal = normalize(turn);
  const first = sub(points[0], centroid);
  const u = normalize(sub(first, scale(normal, dot(first, normal))));
  const v = cross(normal, u);
  // x² + y² + D·x + E·y + F = 0 by least squares; the centre is (−D/2, −E/2).
  const sums = { xx: 0, xy: 0, yy: 0, x: 0, y: 0, xr: 0, yr: 0, r: 0 };
  for (const point of points) {
    const offset = sub(point, centroid);
    const x = dot(offset, u);
    const y = dot(offset, v);
    const r = x * x + y * y;
    sums.xx += x * x;
    sums.xy += x * y;
    sums.yy += y * y;
    sums.x += x;
    sums.y += y;
    sums.xr += x * r;
    sums.yr += y * r;
    sums.r += r;
  }
  const a = [
    [sums.xx, sums.xy, sums.x],
    [sums.xy, sums.yy, sums.y],
    [sums.x, sums.y, points.length],
  ];
  const b = [-sums.xr, -sums.yr, -sums.r];
  const d = det3(a);
  if (Math.abs(d) < 1e-18) throw new Error("the points do not bend round a circle");
  const solve = (column) => det3(a.map((row, i) => row.map((value, j) => (j === column ? b[i] : value)))) / d;
  const [dx, ey, f] = [solve(0), solve(1), solve(2)];
  const cx = -dx / 2;
  const cy = -ey / 2;
  return { centre: add(centroid, add(scale(u, cx), scale(v, cy))), radius: Math.sqrt(Math.max(0, cx * cx + cy * cy - f)), normal };
}
