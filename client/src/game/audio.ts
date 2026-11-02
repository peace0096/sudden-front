// All sound is synthesised with Web Audio; there are no sample files.
import { WeaponKind } from "../gen/game_pb";

export class GameAudio {
  private readonly ctx: AudioContext | null;
  private readonly master: GainNode | null = null;
  private readonly noise: AudioBuffer | null = null;

  constructor() {
    try {
      this.ctx = new AudioContext();
    } catch {
      this.ctx = null;
      return;
    }
    this.master = this.ctx.createGain();
    this.master.gain.value = 0.55;
    this.master.connect(this.ctx.destination);
    const length = this.ctx.sampleRate;
    this.noise = this.ctx.createBuffer(1, length, this.ctx.sampleRate);
    const data = this.noise.getChannelData(0);
    for (let i = 0; i < length; i++) data[i] = Math.random() * 2 - 1;
  }

  /** Browsers start audio suspended until the user interacts with the page. */
  resume(): void {
    if (this.ctx?.state === "suspended") void this.ctx.resume();
  }

  private output(gain: number, pan: number): AudioNode | null {
    if (!this.ctx || !this.master) return null;
    const node = this.ctx.createGain();
    node.gain.value = gain;
    const panner = this.ctx.createStereoPanner();
    panner.pan.value = Math.max(-1, Math.min(1, pan));
    node.connect(panner).connect(this.master);
    return node;
  }

  private noiseBurst(out: AudioNode, type: BiquadFilterType, freq: number, level: number, duration: number, delay = 0): void {
    if (!this.ctx || !this.noise) return;
    const t = this.ctx.currentTime + delay;
    const src = this.ctx.createBufferSource();
    src.buffer = this.noise;
    const filter = this.ctx.createBiquadFilter();
    filter.type = type;
    filter.frequency.value = freq;
    const env = this.ctx.createGain();
    env.gain.setValueAtTime(level, t);
    env.gain.exponentialRampToValueAtTime(0.001, t + duration);
    src.connect(filter).connect(env).connect(out);
    src.start(t, Math.random() * 0.8, duration + 0.02);
  }

  private tone(out: AudioNode, type: OscillatorType, from: number, to: number, level: number, duration: number, delay = 0): void {
    if (!this.ctx) return;
    const t = this.ctx.currentTime + delay;
    const osc = this.ctx.createOscillator();
    osc.type = type;
    osc.frequency.setValueAtTime(from, t);
    osc.frequency.exponentialRampToValueAtTime(Math.max(to, 1), t + duration);
    const env = this.ctx.createGain();
    env.gain.setValueAtTime(level, t);
    env.gain.exponentialRampToValueAtTime(0.001, t + duration);
    osc.connect(env).connect(out);
    osc.start(t);
    osc.stop(t + duration + 0.02);
  }

  gunshot(kind: WeaponKind, distance: number, pan: number): void {
    const gain = 1 / (1 + distance / 10);
    const out = this.output(gain, pan);
    if (!out) return;
    const near = distance < 25;
    const bright = near ? 1 : 0.33;
    switch (kind) {
      case WeaponKind.SNIPER:
      case WeaponKind.MAGNUM:
        this.noiseBurst(out, "lowpass", 6500 * bright, 1.0, 0.32);
        this.noiseBurst(out, "highpass", 2200, near ? 0.5 : 0.12, 0.05);
        this.tone(out, "triangle", 120, 32, 1.0, 0.3);
        break;
      case WeaponKind.MACHINE_GUN:
        this.noiseBurst(out, "lowpass", 3600 * bright, 0.85, 0.09);
        this.tone(out, "triangle", 125, 42, 0.95, 0.1);
        break;
      case WeaponKind.PISTOL:
      case WeaponKind.PISTOL_PLUS:
        this.noiseBurst(out, "lowpass", 4800 * bright, 0.7, 0.07);
        this.noiseBurst(out, "highpass", 2800, near ? 0.3 : 0.06, 0.03);
        this.tone(out, "triangle", 260, 90, 0.55, 0.07);
        break;
      case WeaponKind.SHOTGUN:
        this.noiseBurst(out, "lowpass", 3400 * bright, 1.0, 0.18);
        this.tone(out, "triangle", 110, 36, 1.0, 0.18);
        break;
      case WeaponKind.TASER:
        this.noiseBurst(out, "highpass", 3800, near ? 0.6 : 0.12, 0.22);
        this.tone(out, "sawtooth", 880, 640, 0.25, 0.2);
        break;
      case WeaponKind.MISSILE:
        this.noiseBurst(out, "lowpass", 1800 * bright, 0.9, 0.4);
        this.tone(out, "sawtooth", 320, 60, 0.3, 0.35);
        break;
      case WeaponKind.KNIFE:
        // A swing, not a shot: quieter and whooshing.
        this.noiseBurst(out, "bandpass", 1400, 0.45 * (near ? 1 : 0.2), 0.16);
        break;
      case WeaponKind.SMG:
      case WeaponKind.AKIMBO_SMG:
        this.noiseBurst(out, "lowpass", 7000 * bright, 0.55, 0.06);
        this.noiseBurst(out, "highpass", 3200, near ? 0.3 : 0.06, 0.025);
        this.tone(out, "triangle", 220, 80, 0.45, 0.06);
        break;
      default:
        this.noiseBurst(out, "lowpass", 5200 * bright, 0.85, 0.11);
        this.noiseBurst(out, "highpass", 2400, near ? 0.34 : 0.08, 0.035);
        this.tone(out, "triangle", 165, 48, 0.85, 0.12);
    }
  }

  explosion(distance: number, pan: number): void {
    const out = this.output(1 / (1 + distance / 18), pan);
    if (!out) return;
    this.noiseBurst(out, "lowpass", distance < 30 ? 2600 : 900, 1.0, 0.7);
    this.tone(out, "sine", 90, 28, 1.0, 0.7);
  }

  /** The two clacks of working a rifle bolt. */
  bolt(): void {
    const out = this.output(0.6, 0);
    if (!out) return;
    this.noiseBurst(out, "bandpass", 1700, 0.7, 0.05, 0.35);
    this.noiseBurst(out, "bandpass", 2400, 0.7, 0.05, 0.75);
  }

  dryFire(): void {
    const out = this.output(0.6, 0);
    if (out) this.noiseBurst(out, "highpass", 3200, 0.5, 0.02);
  }

  reload(duration: number): void {
    const out = this.output(0.6, 0);
    if (!out) return;
    for (const [at, freq] of [[0.11, 1500], [0.41, 2100], [0.66, 1300], [0.84, 1800], [0.9, 2500]]) {
      this.noiseBurst(out, "bandpass", freq, 0.65, 0.05, at * duration);
    }
  }

  hitConfirm(headshot: boolean): void {
    const out = this.output(0.42, 0);
    if (out) this.tone(out, "square", headshot ? 1650 : 1050, headshot ? 1250 : 850, 0.25, 0.05);
  }

  hurt(): void {
    const out = this.output(0.6, 0);
    if (out) this.noiseBurst(out, "lowpass", 720, 0.9, 0.11);
  }

  footstep(): void {
    const out = this.output(0.14, 0);
    if (out) this.noiseBurst(out, "lowpass", 620, 0.6, 0.06);
  }

  dispose(): void {
    void this.ctx?.close();
  }
}
