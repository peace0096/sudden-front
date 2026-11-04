// Packs sprites into one WebP atlas of equal square cells, row by row from the top left, with
// straight (not premultiplied) alpha. Every cell is white with the shape in its alpha, so the
// runtime colours sprites itself and filtering at a cell's edge never pulls in a dark fringe.
import sharp from "sharp";

/**
 * Kenney's particles are grey shapes on opaque black, made for additive blending: "luminance"
 * makes the grey the alpha. Splats are white shapes with alpha already: "alpha" keeps it.
 */
async function whiteCell(path, size, mode) {
  const image = sharp(path).resize(size, size, { fit: "contain", background: { r: 0, g: 0, b: 0, alpha: 0 } });
  const source = mode === "luminance" ? image.removeAlpha().extractChannel(0) : image.ensureAlpha().extractChannel(3);
  const { data, info } = await source.raw().toBuffer({ resolveWithObject: true });
  const rgba = Buffer.alloc(info.width * info.height * 4, 255);
  for (let i = 0; i < data.length; i++) rgba[i * 4 + 3] = data[i];
  return sharp(rgba, { raw: { width: info.width, height: info.height, channels: 4 } }).png().toBuffer();
}

/** cells: [{ name, path, mode }]; returns the WebP bytes and each cell's [x, y, width, height]. */
export async function buildAtlas({ cellSize, columns, rows, cells }) {
  if (cells.length > columns * rows) throw new Error(`${cells.length} cells do not fit ${columns}×${rows}`);
  const composites = [];
  const rects = {};
  for (const [i, cell] of cells.entries()) {
    const left = (i % columns) * cellSize;
    const top = Math.floor(i / columns) * cellSize;
    composites.push({ input: await whiteCell(cell.path, cellSize, cell.mode), left, top });
    rects[cell.name] = [left, top, cellSize, cellSize];
  }
  const width = columns * cellSize;
  const height = rows * cellSize;
  const bytes = await sharp({ create: { width, height, channels: 4, background: { r: 255, g: 255, b: 255, alpha: 0 } } })
    .composite(composites)
    .webp({ quality: 90, alphaQuality: 100, effort: 6 })
    .toBuffer();
  return { bytes, rects, width, height };
}
