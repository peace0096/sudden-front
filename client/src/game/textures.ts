// Textures painted in code: wall, floor and roof surfaces, signs, the sky and name tags.
import * as THREE from "three";

type Paint = (g: CanvasRenderingContext2D, w: number, h: number) => void;

function canvasTexture(w: number, h: number, paint: Paint, repeat = true): THREE.CanvasTexture {
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  paint(canvas.getContext("2d")!, w, h);
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  if (repeat) texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
  texture.anisotropy = 8;
  return texture;
}

function speckle(g: CanvasRenderingContext2D, w: number, h: number, count: number, alpha: number, size = 2): void {
  for (let i = 0; i < count; i++) {
    const shade = Math.random() < 0.5 ? 0 : 255;
    g.fillStyle = `rgba(${shade},${shade},${shade},${Math.random() * alpha})`;
    g.fillRect(Math.random() * w, Math.random() * h, Math.random() * size + 1, Math.random() * size + 1);
  }
}

function stain(g: CanvasRenderingContext2D, w: number, h: number, count: number, color: string): void {
  for (let i = 0; i < count; i++) {
    const x = Math.random() * w;
    const y = Math.random() * h;
    const r = 20 + Math.random() * 70;
    const gradient = g.createRadialGradient(x, y, 0, x, y, r);
    gradient.addColorStop(0, color);
    gradient.addColorStop(1, "rgba(0,0,0,0)");
    g.fillStyle = gradient;
    g.fillRect(x - r, y - r, r * 2, r * 2);
  }
}

export const brickTexture = () =>
  canvasTexture(512, 512, (g, w, h) => {
    g.fillStyle = "#3b3029";
    g.fillRect(0, 0, w, h);
    const rows = 16;
    const bh = h / rows;
    const bw = bh * 2.4;
    for (let row = 0; row < rows; row++) {
      const offset = row % 2 ? -bw / 2 : 0;
      for (let col = 0; col <= Math.ceil(w / bw) + 1; col++) {
        const v = Math.random();
        const r = 118 + v * 40;
        g.fillStyle = `rgb(${r},${r * 0.68},${r * 0.54})`;
        g.fillRect(col * bw + offset + 1.5, row * bh + 1.5, bw - 3, bh - 3);
      }
    }
    speckle(g, w, h, 3500, 0.1);
    stain(g, w, h, 8, "rgba(20,14,10,0.28)");
  });

export const concreteTexture = () =>
  canvasTexture(512, 512, (g, w, h) => {
    g.fillStyle = "#a79f8e";
    g.fillRect(0, 0, w, h);
    speckle(g, w, h, 9000, 0.08);
    stain(g, w, h, 12, "rgba(60,52,40,0.13)");
    g.strokeStyle = "rgba(50,45,38,0.5)";
    g.lineWidth = 2;
    g.strokeRect(0, 0, w, h);
  });

export const sandTexture = () =>
  canvasTexture(1024, 1024, (g, w, h) => {
    g.fillStyle = "#b5a27c";
    g.fillRect(0, 0, w, h);
    speckle(g, w, h, 42000, 0.09);
    stain(g, w, h, 34, "rgba(96,80,52,0.15)");
    stain(g, w, h, 20, "rgba(235,220,180,0.13)");
  });

export const roofTexture = () =>
  canvasTexture(256, 256, (g, w, h) => {
    g.fillStyle = "#3a3634";
    g.fillRect(0, 0, w, h);
    g.fillStyle = "#26211f";
    for (let i = 0; i < 4; i++) g.fillRect(0, (i * h) / 4, w, 14);
    speckle(g, w, h, 1500, 0.08);
  });

export const shutterTexture = () =>
  canvasTexture(256, 256, (g, w, h) => {
    g.fillStyle = "#3f3f3e";
    g.fillRect(0, 0, w, h);
    for (let y = 0; y < h; y += 10) {
      g.fillStyle = "#232322";
      g.fillRect(0, y, w, 3);
      g.fillStyle = "rgba(255,255,255,0.08)";
      g.fillRect(0, y + 3, w, 2);
    }
    g.fillStyle = "#1c1c1b";
    g.fillRect(0, 0, 8, h);
    g.fillRect(w - 8, 0, 8, h);
    stain(g, w, h, 5, "rgba(90,55,25,0.28)");
  }, false);

export const louverTexture = () =>
  canvasTexture(256, 96, (g, w, h) => {
    g.fillStyle = "#2a2a28";
    g.fillRect(0, 0, w, h);
    for (let y = 4; y < h; y += 8) {
      g.fillStyle = "#b9bdb4";
      g.fillRect(4, y, w - 8, 4);
      g.fillStyle = "#6f736c";
      g.fillRect(4, y + 4, w - 8, 2);
    }
  }, false);

export const windowTexture = () =>
  canvasTexture(256, 160, (g, w, h) => {
    g.fillStyle = "#34281f";
    g.fillRect(0, 0, w, h);
    const panes = 4;
    for (let col = 0; col < panes; col++) {
      for (let row = 0; row < 2; row++) {
        const px = 8 + col * ((w - 8) / panes);
        const py = 8 + row * ((h - 8) / 2);
        const pw = (w - 8) / panes - 8;
        const ph = (h - 8) / 2 - 8;
        const gradient = g.createLinearGradient(px, py, px + pw, py + ph);
        gradient.addColorStop(0, "#1c2a30");
        gradient.addColorStop(0.5, "#3f565d");
        gradient.addColorStop(1, "#141d21");
        g.fillStyle = gradient;
        g.fillRect(px, py, pw, ph);
      }
    }
  }, false);

export const billboardTexture = () =>
  canvasTexture(1024, 205, (g, w, h) => {
    g.fillStyle = "#dfd8bc";
    g.fillRect(0, 0, w, h);
    g.fillStyle = "#274b6d";
    g.fillRect(0, 0, 370, h);
    g.fillStyle = "#f0ead2";
    g.font = "bold 70px Arial";
    g.fillText("SUPPLY", 34, 90);
    g.font = "bold 46px Arial";
    g.fillText("YARD 07", 34, 155);
    g.fillStyle = "#a33a27";
    g.fillRect(370, 0, 250, h);
    g.fillStyle = "#f0ead2";
    g.font = "bold 96px Arial";
    g.fillText("24H", 410, 135);
    g.fillStyle = "#2b2a25";
    g.font = "bold 44px Arial";
    g.fillText("LOGISTICS", 650, 90);
    g.font = "32px Arial";
    g.fillText("CARGO · FREIGHT", 650, 145);
    speckle(g, w, h, 5000, 0.09);
    stain(g, w, h, 8, "rgba(60,40,20,0.25)");
    g.strokeStyle = "#1e1a15";
    g.lineWidth = 10;
    g.strokeRect(0, 0, w, h);
  }, false);

export const skyTexture = () =>
  canvasTexture(1024, 512, (g, w, h) => {
    const gradient = g.createLinearGradient(0, 0, 0, h);
    gradient.addColorStop(0, "#3f7fc4");
    gradient.addColorStop(0.42, "#9cc6e4");
    gradient.addColorStop(0.5, "#dde7e8");
    gradient.addColorStop(1, "#c8c4b4");
    g.fillStyle = gradient;
    g.fillRect(0, 0, w, h);
    for (let i = 0; i < 90; i++) {
      const cx = Math.random() * w;
      const y = 60 + Math.random() * 180;
      const r = 14 + Math.random() * 48;
      // Drawn once per side of the wrap so clouds cross the seam cleanly.
      for (const x of [cx - w, cx, cx + w]) {
        const cloud = g.createRadialGradient(x, y, 0, x, y, r);
        cloud.addColorStop(0, "rgba(255,255,255,0.55)");
        cloud.addColorStop(1, "rgba(255,255,255,0)");
        g.fillStyle = cloud;
        g.save();
        g.translate(x, y);
        g.scale(1.9, 0.6);
        g.translate(-x, -y);
        g.fillRect(x - r, y - r, r * 2, r * 2);
        g.restore();
      }
    }
  });

export function nameTexture(name: string, color: string): THREE.CanvasTexture {
  return canvasTexture(256, 48, (g, w, h) => {
    g.font = "bold 28px 'Malgun Gothic', sans-serif";
    g.textAlign = "center";
    g.textBaseline = "middle";
    g.strokeStyle = "rgba(0,0,0,0.85)";
    g.lineWidth = 4;
    g.strokeText(name, w / 2, h / 2);
    g.fillStyle = color;
    g.fillText(name, w / 2, h / 2);
  }, false);
}

/**
 * Big Head mode: the King's crown for a sprite over the head, gold with three points, a band and
 * jewels, on a clear background.
 */
export function crownTexture(): THREE.CanvasTexture {
  return canvasTexture(128, 96, (g, w, h) => {
    const gold = g.createLinearGradient(0, 10, 0, h - 8);
    gold.addColorStop(0, "#fff3b0");
    gold.addColorStop(0.45, "#f4c430");
    gold.addColorStop(1, "#a8700c");
    g.lineJoin = "round";
    g.lineWidth = 5;
    g.strokeStyle = "#3a2504";
    g.beginPath();
    g.moveTo(16, h - 12);
    g.lineTo(10, 30);
    g.lineTo(38, 54);
    g.lineTo(w / 2, 12);
    g.lineTo(w - 38, 54);
    g.lineTo(w - 10, 30);
    g.lineTo(w - 16, h - 12);
    g.closePath();
    g.fillStyle = gold;
    g.fill();
    g.stroke();
    g.fillStyle = "#c8901a";
    g.fillRect(16, h - 30, w - 32, 16);
    g.strokeRect(16, h - 30, w - 32, 16);
    const jewels: [number, number, number, string][] = [
      [10, 30, 7, "#ff3b4a"],
      [w / 2, 12, 8, "#ff3b4a"],
      [w - 10, 30, 7, "#ff3b4a"],
      [40, h - 22, 5, "#4ad0ff"],
      [w / 2, h - 22, 6, "#ff3b4a"],
      [w - 40, h - 22, 5, "#4ad0ff"],
    ];
    for (const [x, y, r, color] of jewels) {
      g.beginPath();
      g.arc(x, y, r, 0, Math.PI * 2);
      g.fillStyle = color;
      g.fill();
      g.lineWidth = 2.5;
      g.stroke();
    }
  }, false);
}
