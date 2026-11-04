// glTF models arrive with MeshStandardMaterial. The game lights everything with Lambert, which is
// cheaper on integrated graphics and keeps the number of shader programs down, so models are
// converted and every set of identical materials becomes one shared material.
import * as THREE from "three";

/** A texture this small is a colour palette: each texel is one flat colour that must not blend. */
const PALETTE_MAX_SIZE = 64;

const lamberts = new Map<string, THREE.MeshLambertMaterial>();

/** Swaps every standard or physical material under root for a shared Lambert equivalent. */
export function toLambert(root: THREE.Object3D): void {
  root.traverse((object) => {
    if (!(object instanceof THREE.Mesh)) return;
    object.material = Array.isArray(object.material) ? object.material.map(lambertFor) : lambertFor(object.material);
  });
}

/** Frees the shared materials on the GPU. They stay usable and are set up again when next drawn. */
export function disposeLamberts(): void {
  for (const material of lamberts.values()) material.dispose();
}

// MeshPhysicalMaterial extends MeshStandardMaterial, so both are converted here.
function lambertFor(material: THREE.Material): THREE.Material {
  if (!(material instanceof THREE.MeshStandardMaterial)) return material;
  const key = [
    material.map?.uuid ?? "",
    material.color.toArray().join(","),
    material.emissive.toArray().join(","),
    material.emissiveIntensity,
    material.emissiveMap?.uuid ?? "",
    material.vertexColors,
    material.side,
    material.transparent,
    material.alphaTest,
    material.opacity,
    // The glTF loader turns depth writes off for blended materials.
    material.depthWrite,
    material.flatShading,
  ].join("|");
  let lambert = lamberts.get(key);
  if (!lambert) {
    lambert = new THREE.MeshLambertMaterial({
      name: material.name,
      map: material.map,
      color: material.color,
      emissive: material.emissive,
      emissiveIntensity: material.emissiveIntensity,
      emissiveMap: material.emissiveMap,
      vertexColors: material.vertexColors,
      side: material.side,
      transparent: material.transparent,
      alphaTest: material.alphaTest,
      opacity: material.opacity,
      depthWrite: material.depthWrite,
      flatShading: material.flatShading,
    });
    for (const texture of [material.map, material.emissiveMap]) {
      if (texture && isPalette(texture)) samplePalette(texture);
    }
    lamberts.set(key, lambert);
  }
  return lambert;
}

function isPalette(texture: THREE.Texture): boolean {
  const image = texture.image as { width?: number; height?: number } | null;
  return !!image?.width && !!image.height && image.width <= PALETTE_MAX_SIZE && image.height <= PALETTE_MAX_SIZE;
}

/** Nearest texels, without mipmaps, which would mix neighbouring palette colours in the distance. */
function samplePalette(texture: THREE.Texture): void {
  if (texture.magFilter === THREE.NearestFilter && texture.minFilter === THREE.NearestFilter) return;
  texture.magFilter = THREE.NearestFilter;
  texture.minFilter = THREE.NearestFilter;
  texture.generateMipmaps = false;
  texture.needsUpdate = true;
}
