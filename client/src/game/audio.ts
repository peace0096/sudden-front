// All sound is synthesised with Web Audio; there are no sample files. Weapon sounds are layered
// offline (crack, blast, body, mechanism, echo) into a few randomised takes per sound, once per
// sample rate, so a shot in play is a single buffer playback.
import { WeaponKind } from "../gen/game_pb";

interface Band {
  type: "lowpass" | "highpass" | "bandpass";
  hz: number;
  q?: number;
  /** Frequency the filter sweeps to, exponentially, over `over` seconds. */
  to?: number;
  over?: number;
}

const lp = (hz: number, to?: number, over?: number): Band => ({ type: "lowpass", hz, to, over });
const hp = (hz: number): Band => ({ type: "highpass", hz });
const bp = (hz: number, q: number, to?: number, over?: number): Band => ({ type: "bandpass", hz, q, to, over });

/** Builds one take of a sound into an offline context. */
class Synth {
  /** Clean bus to the output and the echo: mechanical noises, and the blast once saturated. */
  readonly dry: GainNode;
  /**
   * Saturating bus for the blast layers: unity gain for small signals, rounding off towards ±1
   * beyond 1. Layers pushed well past 1 clip for their first milliseconds, which gives the punch.
   */
  readonly hot: GainNode;
  private readonly post: BiquadFilterNode;
  private readonly noiseBuffer: AudioBuffer;

  constructor(readonly ctx: BaseAudioContext) {
    // Cut DC and sub-bass so they don't eat headroom.
    this.post = ctx.createBiquadFilter();
    this.post.type = "highpass";
    this.post.frequency.value = 30;
    this.post.connect(ctx.destination);
    this.dry = ctx.createGain();
    this.dry.connect(this.post);
    // The curve spans inputs of ±4, so the bus scales its input by a quarter.
    const curve = new Float32Array(1024);
    for (let i = 0; i < curve.length; i++) curve[i] = Math.tanh(4 * ((2 * i) / (curve.length - 1) - 1));
    const shaper = ctx.createWaveShaper();
    shaper.curve = curve;
    shaper.oversample = "4x";
    shaper.connect(this.dry);
    this.hot = ctx.createGain();
    this.hot.gain.value = 0.25;
    this.hot.connect(shaper);
    this.noiseBuffer = ctx.createBuffer(1, 2 * ctx.sampleRate, ctx.sampleRate);
    const data = this.noiseBuffer.getChannelData(0);
    for (let i = 0; i < data.length; i++) data[i] = Math.random() * 2 - 1;
  }

  /** `x` jittered by up to ±`amount` (a fraction), so no two takes are the same. */
  vary(x: number, amount = 0.08): number {
    return x * (1 + (Math.random() * 2 - 1) * amount);
  }

  /** Filtered noise with a linear attack and an exponential decay; `level` is roughly its peak. */
  noise(dest: AudioNode, at: number, level: number, decay: number, bands: Band[], attack = 0): void {
    const { ctx } = this;
    const nyquist = ctx.sampleRate / 2;
    const src = ctx.createBufferSource();
    src.buffer = this.noiseBuffer;
    src.loop = true;
    let node: AudioNode = src;
    let share = 1; // Fraction of the noise power the filters pass.
    for (const band of bands) {
      const filter = ctx.createBiquadFilter();
      filter.type = band.type;
      if (band.q !== undefined) filter.Q.value = band.q;
      filter.frequency.setValueAtTime(band.hz, at);
      if (band.to !== undefined) filter.frequency.exponentialRampToValueAtTime(band.to, at + (band.over ?? decay * 3));
      const width = band.type === "lowpass" ? band.hz : band.type === "highpass" ? nyquist - band.hz : band.hz / (band.q ?? 1);
      share *= Math.min(1, width / nyquist);
      node = node.connect(filter);
    }
    const peak = level / (1.73 * Math.sqrt(Math.max(share, 1e-3)));
    const env = ctx.createGain();
    env.gain.setValueAtTime(attack > 0 ? 0 : peak, at);
    if (attack > 0) env.gain.linearRampToValueAtTime(peak, at + attack);
    env.gain.setTargetAtTime(0, at + attack, decay);
    node.connect(env).connect(dest);
    src.start(at, Math.random() * 1.5);
    src.stop(at + attack + decay * 7);
  }

  /** A tone that drops from `from` to `to` Hz over `sweep` seconds: a blast's low thump, or a metal mode. */
  tone(dest: AudioNode, at: number, level: number, from: number, to: number, sweep: number, decay: number, type: OscillatorType = "sine"): void {
    const { ctx } = this;
    const osc = ctx.createOscillator();
    osc.type = type;
    osc.frequency.setValueAtTime(from, at);
    if (to !== from) osc.frequency.exponentialRampToValueAtTime(to, at + sweep);
    const env = ctx.createGain();
    env.gain.setValueAtTime(level, at);
    env.gain.setTargetAtTime(0, at, decay);
    osc.connect(env).connect(dest);
    osc.start(at);
    osc.stop(at + decay * 7);
  }

  /** A struck steel part: a hard tick ringing at a few inharmonic modes. */
  ring(at: number, level: number, modes: number[], decay: number): void {
    this.noise(this.dry, at, level, 0.0012, [hp(2500)]);
    modes.forEach((hz, i) => {
      const f = this.vary(hz, 0.03);
      this.tone(this.dry, at, level / (1 + i * 0.5), f, f, 0, decay / (1 + i * 0.3));
    });
  }

  /** A supersonic bullet's N-shaped shock wave, `ms` long, and its bounce off the ground. */
  crack(at: number, level: number, ms: number): void {
    const length = ms / 1000;
    const bounce = this.vary(0.003, 0.3);
    const wave = (t: number) => (t >= 0 && t < length ? 1 - (2 * t) / length : 0);
    this.draw(at, bounce + length, (t) => level * (wave(t) + 0.5 * wave(t - bounce)));
  }

  /** The muzzle's blast front (a Friedlander wave): a sharp overpressure that swings negative. */
  front(at: number, level: number, ms: number): void {
    const length = ms / 1000;
    this.draw(at, length * 8, (t) => level * (1 - t / length) * Math.exp(-t / length));
  }

  /** Plays a pressure wave drawn sample by sample into the saturating bus. */
  private draw(at: number, seconds: number, wave: (t: number) => number): void {
    const { ctx } = this;
    const buffer = ctx.createBuffer(1, Math.max(1, Math.round(seconds * ctx.sampleRate)), ctx.sampleRate);
    const data = buffer.getChannelData(0);
    for (let i = 0; i < data.length; i++) data[i] = wave(i / ctx.sampleRate);
    const src = ctx.createBufferSource();
    src.buffer = buffer;
    src.connect(this.hot);
    src.start(at);
  }

  /**
   * Sends the dry mix through a generated reflection pattern: a diffuse decay that darkens as it
   * fades, plus `echoes` discrete echoes rolling back from far walls and hills.
   */
  tail(level: number, seconds: number, decay: number, bright: number, echoes: number): void {
    const { ctx } = this;
    const rate = ctx.sampleRate;
    const response = ctx.createBuffer(1, Math.round(seconds * rate), rate);
    const data = response.getChannelData(0);
    // The later an echo returns, the farther it travelled and the more it is smeared into a roll.
    for (let e = 0; e < echoes; e++) {
      const at = 0.03 + Math.random() * Math.random() * seconds * 0.6;
      const start = Math.round(at * rate);
      const length = Math.round((0.004 + at * (0.05 + Math.random() * 0.15)) * rate);
      const amp = (1 + Math.random() * 2) * Math.exp(-at / (decay * 1.5));
      for (let i = 0; i < length && start + i < data.length; i++) data[start + i] += amp * (Math.random() * 2 - 1) * Math.sin((Math.PI * i) / length);
    }
    const onset = 0.01 + decay * 0.1;
    let low1 = 0;
    let low2 = 0;
    let energy = 0;
    for (let i = 0; i < data.length; i++) {
      const t = i / rate;
      const x = data[i] + (Math.random() * 2 - 1) * Math.exp(-t / decay) * Math.min(1, t / onset);
      // Air and ground soak up the highs first, so the tail darkens as it fades.
      const k = 1 - Math.exp((-2 * Math.PI * (150 + bright * Math.exp(-t / decay))) / rate);
      low1 += k * (x - low1);
      low2 += k * (low1 - low2);
      data[i] = low2;
      energy += low2 * low2;
    }
    const gain = level / Math.sqrt(energy || 1);
    for (let i = 0; i < data.length; i++) data[i] *= gain;
    const reverb = ctx.createConvolver();
    reverb.normalize = false;
    reverb.buffer = response;
    // Keep the bass out of the echo, or it swells into a boom louder than the shot itself.
    const send = ctx.createBiquadFilter();
    send.type = "highpass";
    send.frequency.value = 250;
    this.dry.connect(send).connect(reverb).connect(this.post);
  }
}

/** A spent case landing and bouncing; `modes` are its ringing frequencies. */
function brass(s: Synth, at: number, level: number, modes: number[]): void {
  for (let i = 0; i < 3; i++) {
    s.ring(at, level, modes, 0.04);
    at += s.vary(0.09 / (i + 1), 0.3);
    level *= 0.4;
  }
}

const CASE_9MM = [4300, 7600, 10400];
const CASE_338 = [2600, 4700, 6900];

interface Recipe {
  /** Seconds rendered; the silent end is trimmed afterwards. */
  length: number;
  /** Peak level of every take, which sets the sounds' relative loudness. */
  peak: number;
  /** Rapid-fire weapons get more takes so that sustained fire doesn't sound looped. */
  takes: number;
  build(s: Synth): void;
}

/** In render order: the normal-mode weapons first, since takes become playable as they finish. */
const RECIPES = {
  rifle: {
    length: 0.9,
    peak: 0.9,
    takes: 6,
    build(s) {
      // M416, 5.56 mm: a sharp supersonic crack over a tight, mid-focused bang.
      s.crack(0, 1.6, s.vary(0.3));
      s.noise(s.hot, 0, 1.2, s.vary(0.003), [hp(3000)]);
      s.front(0, 1.3, s.vary(1.4));
      s.noise(s.hot, 0, 2.2, s.vary(0.012), [bp(s.vary(3000), 0.9, 800, 0.04)]);
      s.tone(s.hot, 0, 0.32, s.vary(165), 58, 0.06, s.vary(0.03));
      s.noise(s.hot, 0, 0.16, s.vary(0.04), [lp(400)]);
      // The bolt carrier slams into the buffer, whose spring twangs, then runs back into battery.
      s.ring(s.vary(0.022), 0.16, [1750, 2900, 4400], 0.02);
      s.tone(s.dry, s.vary(0.026), 0.05, s.vary(1330), 1270, 0.08, 0.05);
      s.ring(s.vary(0.065), 0.13, [2100, 3500, 5300], 0.015);
      s.tail(0.24, 0.9, 0.2, 5000, 3);
    },
  },
  pistol: {
    length: 0.9,
    peak: 0.75,
    takes: 4,
    build(s) {
      // PT-9, 9 mm: a round pop, the slide's clack as it runs back into battery, a quick slap echo.
      s.noise(s.hot, 0, 0.9, s.vary(0.0025), [hp(2200)]);
      s.front(0, 1.4, s.vary(1.1));
      s.noise(s.hot, 0, 2.2, s.vary(0.01), [bp(s.vary(2000), 0.9, 700, 0.035)]);
      s.tone(s.hot, 0, 0.42, s.vary(200), 78, 0.04, s.vary(0.02));
      s.noise(s.hot, 0, 0.1, s.vary(0.03), [lp(450)]);
      s.ring(s.vary(0.012), 0.12, [1900, 3300], 0.01);
      s.ring(s.vary(0.05), 0.22, [1900, 3100, 4800], 0.018);
      brass(s, s.vary(0.45), 0.04, CASE_9MM);
      s.tail(0.3, 0.5, 0.09, 5500, 2);
    },
  },
  sniper: {
    length: 2.4,
    peak: 1,
    takes: 3,
    build(s) {
      // AWP, .338: the loudest gun. A huge crack and a deep boom that rolls away in echoes.
      s.crack(0, 2, s.vary(0.45));
      s.noise(s.hot, 0, 1.4, s.vary(0.005), [hp(2500)]);
      s.front(0, 2.5, s.vary(3));
      s.noise(s.hot, 0, 3.6, s.vary(0.026), [bp(s.vary(2400), 0.6, 330, 0.08)]);
      s.tone(s.hot, 0, 0.7, s.vary(92), 30, 0.2, s.vary(0.09));
      s.noise(s.hot, 0, 0.4, s.vary(0.12), [lp(260)]);
      s.tail(0.55, 2.3, 0.5, 4500, 9);
    },
  },
  bolt: {
    length: 1.2,
    peak: 0.4,
    takes: 3,
    build(s) {
      // Handle up; bolt back, sliding steel ending in the extractor's clack; forward, chambering
      // the next round; handle down to lock. Then the ejected case lands.
      s.ring(0, 0.5, [2400, 3900, 6100], 0.015);
      s.noise(s.dry, s.vary(0.05), 0.35, 0.04, [bp(2600, 1.5, 3600, 0.12)], 0.05);
      s.ring(s.vary(0.16), 0.8, [1500, 2650, 4200], 0.025);
      s.noise(s.dry, s.vary(0.3), 0.3, 0.03, [bp(3400, 1.5, 2200, 0.1)], 0.04);
      s.ring(s.vary(0.39), 0.85, [1300, 2300, 3700], 0.025);
      s.ring(s.vary(0.47), 0.6, [1900, 3100, 5200], 0.018);
      brass(s, s.vary(0.62), 0.12, CASE_338);
      s.tail(0.25, 0.4, 0.07, 5000, 0);
    },
  },
  machineGun: {
    length: 0.8,
    peak: 0.85,
    takes: 6,
    build(s) {
      // M249, 5.56 mm belt-fed: a duller crack over a heavier, deeper bang; the belt rattles through.
      s.crack(0, 0.7, s.vary(0.32));
      s.noise(s.hot, 0, 0.4, s.vary(0.002), [hp(2500)]);
      s.front(0, 1.6, s.vary(1.8));
      s.noise(s.hot, 0, 2.8, s.vary(0.014), [bp(s.vary(1500), 0.7, 400, 0.05)]);
      s.tone(s.hot, 0, 0.75, s.vary(135), 46, 0.09, s.vary(0.04));
      s.noise(s.hot, 0, 0.35, s.vary(0.06), [lp(320)]);
      // The heavy bolt and feed tray, then the belt's links jingling.
      s.noise(s.dry, s.vary(0.03), 0.25, 0.008, [bp(1200, 2)]);
      s.ring(s.vary(0.032), 0.2, [900, 1550, 2600], 0.025);
      let link = s.vary(0.016, 0.2);
      for (let i = 0; i < 4; i++) {
        s.ring(link, s.vary(0.08, 0.3), [s.vary(4200, 0.15), s.vary(6100, 0.15)], 0.006);
        link += s.vary(0.012, 0.4);
      }
      s.tail(0.26, 0.8, 0.16, 2800, 3);
    },
  },
  smg: {
    length: 0.5,
    peak: 0.8,
    takes: 6,
    build(s) {
      // UZI, 9 mm open bolt: a light, dry pap under the clatter of the heavy bolt slamming around
      // a tinny stamped-steel receiver.
      s.noise(s.hot, 0, 1.4, s.vary(0.003), [hp(1500)]);
      s.front(0, 1.1, s.vary(0.9));
      s.noise(s.hot, 0, 1.8, s.vary(0.007), [bp(s.vary(3400), 1.1, 1300, 0.025)]);
      s.tone(s.hot, 0, 0.26, s.vary(250), 105, 0.03, s.vary(0.012));
      s.ring(s.vary(0.004, 0.5), 0.4, [2700, 4300, 6500, 8200], 0.03);
      s.ring(s.vary(0.006, 0.5), 0.25, [1100, 1800], 0.01);
      s.ring(s.vary(0.035), 0.14, [3000, 4700], 0.015);
      s.tail(0.25, 0.35, 0.06, 6000, 0);
    },
  },
  shotgun: {
    length: 1.6,
    peak: 0.9,
    takes: 3,
    build(s) {
      // SG-12, 12 gauge: no sharp crack, but a wide, full-bodied boom.
      s.noise(s.hot, 0, 0.9, s.vary(0.0025), [hp(1800)]);
      s.front(0, 2, s.vary(4));
      s.noise(s.hot, 0, 2.8, s.vary(0.026), [lp(s.vary(4000), 450, 0.07)]);
      s.noise(s.hot, 0, 1, s.vary(0.04), [bp(500, 0.6)]);
      s.tone(s.hot, 0, 0.7, s.vary(92), 36, 0.12, s.vary(0.07));
      s.noise(s.hot, 0, 0.4, s.vary(0.11), [lp(300)]);
      s.tail(0.45, 1.6, 0.38, 3200, 6);
    },
  },
  pump: {
    length: 1,
    peak: 0.5,
    takes: 3,
    build(s) {
      // Fore-end racked back, unlocking with a clack, then slammed forward into a solid chunk.
      s.noise(s.dry, 0, 0.45, 0.035, [bp(1700, 1.2, 2400, 0.08)], 0.02);
      s.ring(s.vary(0.075), 0.85, [1100, 1900, 3200], 0.03);
      s.noise(s.dry, s.vary(0.15), 0.4, 0.025, [bp(2400, 1.2, 1500, 0.06)], 0.015);
      const lock = s.vary(0.21);
      s.ring(lock, 1, [950, 1700, 2900, 4300], 0.035);
      s.tone(s.dry, lock, 0.5, 240, 150, 0.03, 0.02);
      // The empty plastic hull lands with a dull tock.
      const hull = s.vary(0.55);
      s.noise(s.dry, hull, 0.2, 0.01, [bp(900, 3)]);
      s.noise(s.dry, hull + s.vary(0.08), 0.08, 0.01, [bp(1000, 3)]);
      s.tail(0.25, 0.4, 0.07, 5000, 0);
    },
  },
  magnum: {
    length: 1.5,
    peak: 0.95,
    takes: 3,
    build(s) {
      // RV-44, .44 Magnum: a hard crack with gas hissing from the cylinder gap, a heavy boom and a
      // long echo. No slide; instead the cylinder ratchets round as the hammer is cocked.
      s.crack(0, 1.8, s.vary(0.3));
      s.noise(s.hot, 0, 1.3, s.vary(0.01), [hp(3500)]);
      s.front(0, 1.8, s.vary(2.5));
      s.noise(s.hot, 0, 2.8, s.vary(0.018), [bp(s.vary(1700), 0.7, 320, 0.06)]);
      s.tone(s.hot, 0, 0.6, s.vary(130), 45, 0.15, s.vary(0.05));
      s.noise(s.hot, 0, 0.28, s.vary(0.07), [lp(300)]);
      s.ring(0.002, 0.03, [2650, 4150], 0.1);
      const cock = s.vary(0.45);
      s.ring(cock, 0.06, [3100, 5200], 0.008);
      s.ring(cock + s.vary(0.06), 0.09, [2300, 3800, 5600], 0.012);
      s.tail(0.36, 1.3, 0.3, 3800, 4);
    },
  },
  missile: {
    length: 2.4,
    peak: 0.85,
    takes: 3,
    build(s) {
      // ML-9: the ejection charge thumps out of the tube, then the motor roars away, falling in pitch.
      s.tone(s.hot, 0, 1, s.vary(85), 38, 0.1, 0.07);
      s.noise(s.hot, 0, 1.4, 0.03, [lp(900)]);
      s.noise(s.hot, 0, 0.8, 0.004, [hp(2000)]);
      const ignite = s.vary(0.06);
      s.noise(s.hot, ignite, 1, 0.005, [hp(1500)]);
      s.noise(s.dry, ignite, 0.6, s.vary(0.35), [bp(2200, 0.8, 450, 1)], 0.05);
      s.noise(s.dry, ignite, 0.55, s.vary(0.4), [lp(700, 200, 1)], 0.06);
      s.tone(s.dry, ignite, 0.12, 320, 60, 0.6, 0.3, "sawtooth");
      s.tail(0.3, 1.2, 0.3, 3000, 3);
    },
  },
  taser: {
    length: 0.7,
    peak: 0.55,
    takes: 3,
    build(s) {
      // ZAP-1: a gas pop launches the probes, then the arc buzzes and crackles.
      const { ctx } = s;
      s.noise(s.dry, 0, 0.5, 0.01, [bp(900, 0.8)]);
      const end = s.vary(0.42, 0.1);
      // The buzz: a jittery sawtooth, band-passed into a nasal zzz and clipped hard.
      const osc = ctx.createOscillator();
      osc.type = "sawtooth";
      osc.frequency.setValueCurveAtTime(new Float32Array(24).map(() => 85 + Math.random() * 60), 0.02, end - 0.02);
      const band = ctx.createBiquadFilter();
      band.type = "bandpass";
      band.frequency.value = 2600;
      band.Q.value = 1.2;
      const env = ctx.createGain();
      env.gain.setValueAtTime(0, 0.02);
      env.gain.linearRampToValueAtTime(3, 0.03);
      env.gain.setValueAtTime(3, end);
      env.gain.linearRampToValueAtTime(0, end + 0.04);
      osc.connect(band).connect(env).connect(s.hot);
      osc.start(0.02);
      osc.stop(end + 0.05);
      // Sparks: snaps at the cartridge's pulse rate, and finer random crackle.
      for (let t = 0.02; t < end; t += s.vary(0.05, 0.4)) s.noise(s.hot, t, s.vary(2.5, 0.3), 0.003, [hp(1500)]);
      for (let i = 0; i < 40; i++) s.noise(s.hot, 0.02 + Math.random() * (end - 0.02), 0.6 + Math.random() * 1.4, 0.0008, [hp(3000)]);
      s.tail(0.25, 0.25, 0.05, 6000, 0);
    },
  },
} satisfies Record<string, Recipe>;

type SoundName = keyof typeof RECIPES;

const SHOT_SOUNDS: Partial<Record<WeaponKind, SoundName>> = {
  [WeaponKind.SNIPER]: "sniper",
  [WeaponKind.MACHINE_GUN]: "machineGun",
  [WeaponKind.SMG]: "smg",
  [WeaponKind.AKIMBO_SMG]: "smg",
  [WeaponKind.PISTOL]: "pistol",
  [WeaponKind.PISTOL_PLUS]: "pistol",
  [WeaponKind.MAGNUM]: "magnum",
  [WeaponKind.SHOTGUN]: "shotgun",
  [WeaponKind.MISSILE]: "missile",
  [WeaponKind.TASER]: "taser",
};

async function render(recipe: Recipe, sampleRate: number): Promise<AudioBuffer> {
  const ctx = new OfflineAudioContext(1, Math.ceil(recipe.length * sampleRate), sampleRate);
  recipe.build(new Synth(ctx));
  const data = (await ctx.startRendering()).getChannelData(0);
  let peak = 0;
  for (const v of data) peak = Math.max(peak, Math.abs(v));
  // Trim the end below -50 dB, so voices finish (and free) early, with a short fade.
  let end = data.length;
  while (end > 1 && Math.abs(data[end - 1]) < peak * 0.003) end--;
  const fade = Math.min(end, Math.round(sampleRate * 0.005));
  const scale = recipe.peak / (peak || 1);
  for (let i = 0; i < end; i++) data[i] *= scale * Math.min(1, (end - i) / fade);
  const take = ctx.createBuffer(1, end, sampleRate);
  take.copyToChannel(data.subarray(0, end), 0);
  return take;
}

/** Rendered takes per sample rate, kept across matches so that each is rendered once. */
const banks = new Map<number, Map<SoundName, AudioBuffer[]>>();
const loads = new Map<number, Promise<void>>();

/** Renders every take at `sampleRate`; a sound is playable as soon as its own takes are done. */
export function loadSounds(sampleRate: number): Promise<void> {
  let load = loads.get(sampleRate);
  if (!load) {
    const bank = banks.get(sampleRate) ?? new Map<SoundName, AudioBuffer[]>();
    banks.set(sampleRate, bank);
    load = (async () => {
      let failed = false;
      for (const [name, recipe] of Object.entries(RECIPES) as [SoundName, Recipe][]) {
        if (bank.has(name)) continue;
        try {
          bank.set(name, await Promise.all(Array.from({ length: recipe.takes }, () => render(recipe, sampleRate))));
        } catch (error) {
          failed = true;
          console.warn(`Could not render the "${name}" sound:`, error);
        }
      }
      // The next match retries whatever failed.
      if (failed) loads.delete(sampleRate);
    })();
    loads.set(sampleRate, load);
  }
  return load;
}

/** Simultaneous rendered sounds, so that heavy fire stays cheap (see `makeRoom`). */
const MAX_VOICES = 48;

interface Voice {
  src: AudioBufferSourceNode;
  out: GainNode;
  /** Distance gain times the take's peak: how loud the voice starts. */
  level: number;
  /** Context time at which it starts playing. */
  start: number;
  /** The player's own sounds, which are never cut short. */
  local: boolean;
}

export class GameAudio {
  private readonly ctx: BaseAudioContext | null;
  private readonly master: GainNode | null = null;
  /** Hit, hurt and dry-fire cues join after the limiter, so heavy fire can't turn them down. */
  private readonly cues: GainNode | null = null;
  private readonly noise: AudioBuffer | null = null;
  private readonly voices = new Set<Voice>();
  private readonly lastTake = new Map<SoundName, number>();
  private akimboLeft = false;

  /** Plays to a new AudioContext unless given a context (an OfflineAudioContext renders the mix). */
  constructor(ctx?: BaseAudioContext) {
    try {
      this.ctx = ctx ?? new AudioContext();
    } catch {
      this.ctx = null;
      return;
    }
    // A limiter keeps many players' sustained fire from clipping. Its automatic makeup gain
    // (about +3.4 dB at these settings) brings the 0.37 bus gain back to an overall 0.55.
    const limiter = this.ctx.createDynamicsCompressor();
    limiter.threshold.value = -6;
    limiter.knee.value = 0;
    limiter.ratio.value = 20;
    limiter.attack.value = 0.002;
    limiter.release.value = 0.15;
    limiter.connect(this.ctx.destination);
    this.master = this.ctx.createGain();
    this.master.gain.value = 0.37;
    this.master.connect(limiter);
    this.cues = this.ctx.createGain();
    this.cues.gain.value = 0.55;
    this.cues.connect(this.ctx.destination);
    const length = this.ctx.sampleRate;
    this.noise = this.ctx.createBuffer(1, length, this.ctx.sampleRate);
    const data = this.noise.getChannelData(0);
    for (let i = 0; i < length; i++) data[i] = Math.random() * 2 - 1;
    // Until the takes are ready (early in the first match) shots use a plain fallback.
    void loadSounds(this.ctx.sampleRate);
  }

  /** Browsers start audio suspended until the user interacts with the page. */
  resume(): void {
    if (this.ctx?.state === "suspended" && this.ctx instanceof AudioContext) void this.ctx.resume();
  }

  /**
   * Whether sounds can be scheduled. A live context that isn't running has a frozen clock, so all
   * sounds scheduled meanwhile would play at once on resume. An offline context renders what was
   * scheduled while it was suspended, so it always counts.
   */
  private running(): boolean {
    return !!this.ctx && (this.ctx.state === "running" || !(this.ctx instanceof AudioContext));
  }

  private output(gain: number, pan: number, bus = this.master): GainNode | null {
    if (!this.ctx || !bus || !this.running()) return null;
    const node = this.ctx.createGain();
    node.gain.value = gain;
    const panner = this.ctx.createStereoPanner();
    panner.pan.value = Math.max(-1, Math.min(1, pan));
    node.connect(panner).connect(bus);
    return node;
  }

  /** Plays a random take of a rendered sound; false only if it isn't rendered yet. */
  private play(name: SoundName, gain: number, pan: number, distance = 0, rate = 1, delay = 0): boolean {
    const ctx = this.ctx;
    const takes = ctx && banks.get(ctx.sampleRate)?.get(name);
    if (!ctx || !takes) return false;
    const level = gain * RECIPES[name].peak;
    const local = distance === 0;
    const out = this.running() && this.makeRoom(ctx, level, local) ? this.output(gain, pan) : null;
    if (!out) return true;
    // Never the same take twice in a row, so automatic fire doesn't sound like a loop.
    let take = Math.floor(Math.random() * takes.length);
    if (take === this.lastTake.get(name)) take = (take + 1) % takes.length;
    this.lastTake.set(name, take);
    const src = ctx.createBufferSource();
    src.buffer = takes[take];
    src.playbackRate.value = rate * (0.97 + Math.random() * 0.06);
    let node: AudioNode = src;
    if (!local) {
      // Air absorbs the highs first, so far shots are duller as well as quieter.
      const air = ctx.createBiquadFilter();
      air.frequency.value = Math.max(800, 20000 * Math.exp(-distance / 25));
      node = src.connect(air);
    }
    node.connect(out);
    const start = ctx.currentTime + delay;
    src.start(start);
    const voice = { src, out, level, start, local };
    this.voices.add(voice);
    src.onended = () => this.voices.delete(voice);
    return true;
  }

  /**
   * At the voice cap, stops the quietest remote voice that has started (its start level, decayed
   * roughly as a shot does) if it is quieter than the new one. False means skip the new voice: a
   * remote one is dropped when nothing can go, while the player's own sounds always play. The
   * player's sounds, and voices still waiting to start (bolt, pump), are never cut.
   */
  private makeRoom(ctx: BaseAudioContext, level: number, local: boolean): boolean {
    if (this.voices.size < MAX_VOICES) return true;
    const now = ctx.currentTime;
    let quietest: Voice | undefined;
    let lowest = local ? Infinity : level;
    for (const voice of this.voices) {
      const loudness = voice.level / (1 + (now - voice.start) / 0.02);
      if (!voice.local && voice.start <= now && loudness < lowest) {
        quietest = voice;
        lowest = loudness;
      }
    }
    if (!quietest) return local;
    this.voices.delete(quietest);
    quietest.out.gain.setTargetAtTime(0, now, 0.01);
    quietest.src.stop(now + 0.05);
    return true;
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
    if (kind === WeaponKind.KNIFE) {
      // A swing, not a shot: quieter and whooshing.
      const out = this.output(gain, pan);
      if (out) this.noiseBurst(out, "bandpass", 1400, 0.45 * (distance < 25 ? 1 : 0.2), 0.16);
      return;
    }
    let rate = 1;
    if (kind === WeaponKind.AKIMBO_SMG) {
      // Two guns firing in turn, one in each hand.
      this.akimboLeft = !this.akimboLeft;
      pan += (this.akimboLeft ? -0.3 : 0.3) / (1 + distance);
      rate = this.akimboLeft ? 0.97 : 1.03;
    }
    if (this.play(SHOT_SOUNDS[kind] ?? "rifle", gain, pan, distance, rate)) return;
    // Not rendered yet: a plain burst.
    const out = this.output(gain, pan);
    if (out) this.noiseBurst(out, "lowpass", 4000 / (1 + distance / 30), 0.8, 0.1);
  }

  explosion(distance: number, pan: number): void {
    const out = this.output(1 / (1 + distance / 18), pan);
    if (!out) return;
    this.noiseBurst(out, "lowpass", distance < 30 ? 2600 : 900, 1.0, 0.7);
    this.tone(out, "sine", 90, 28, 1.0, 0.7);
  }

  /** Working the sniper rifle's bolt after a shot. */
  bolt(): void {
    this.play("bolt", 1, 0, 0, 1, 0.3);
  }

  /** Racking the shotgun's pump after a shot. */
  pump(): void {
    this.play("pump", 1, 0, 0, 1, 0.28);
  }

  dryFire(): void {
    const out = this.output(0.6, 0, this.cues);
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
    const out = this.output(0.42, 0, this.cues);
    if (out) this.tone(out, "square", headshot ? 1650 : 1050, headshot ? 1250 : 850, 0.25, 0.05);
  }

  hurt(): void {
    const out = this.output(0.6, 0, this.cues);
    if (out) this.noiseBurst(out, "lowpass", 720, 0.9, 0.11);
  }

  footstep(): void {
    const out = this.output(0.14, 0);
    if (out) this.noiseBurst(out, "lowpass", 620, 0.6, 0.06);
  }

  dispose(): void {
    if (this.ctx && this.ctx instanceof AudioContext) void this.ctx.close();
  }
}
