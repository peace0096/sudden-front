// Frame cost from the renderer's counters and frame times, for Game.perf() and the ?debug&perf
// overlay. Without that overlay it only stores a timestamp per frame.
import type * as THREE from "three";

/** Frames in the rolling window for the frame time average and 95th percentile. */
const WINDOW = 120;
/** How often the overlay is rewritten. */
const OVERLAY_INTERVAL_MS = 250;

export interface PerfStats {
  /** Draw calls of the last frame, all of its passes together. */
  calls: number;
  triangles: number;
  /** Shader programs compiled and alive. */
  programs: number;
  geometries: number;
  textures: number;
  /** Time between frames over the window, in ms; null until there are any. */
  avgFrameMs: number | null;
  p95FrameMs: number | null;
  /** From the Game's construction to the end of its first rendered frame; null until then. */
  firstFrameMs: number | null;
}

const round = (value: number) => Math.round(value * 10) / 10;

export class PerfMeter {
  private readonly intervals = new Float64Array(WINDOW);
  private filled = 0;
  private next = 0;
  private frames = 0;
  private lastStart = 0;
  private firstFrameMs: number | null = null;
  private readonly overlay: HTMLDivElement | null = null;
  private overlayAt = 0;

  /** `since` is when the Game began construction. */
  constructor(
    private readonly renderer: THREE.WebGLRenderer,
    parent: HTMLElement,
    private readonly since: number,
  ) {
    // A frame renders up to three passes (world, x-ray, view model). The counters add them all up
    // until beginFrame clears them.
    renderer.info.autoReset = false;
    const params = new URLSearchParams(location.search);
    if (params.has("debug") && params.has("perf")) {
      this.overlay = document.createElement("div");
      this.overlay.className = "perf-overlay";
      parent.append(this.overlay);
    }
  }

  beginFrame(now: number): void {
    this.renderer.info.reset();
    this.frames++;
    // The gap after the first frame holds its shader compiles, which firstFrameMs reports instead.
    if (this.frames > 2) {
      this.intervals[this.next] = now - this.lastStart;
      this.next = (this.next + 1) % WINDOW;
      this.filled = Math.min(this.filled + 1, WINDOW);
    }
    this.lastStart = now;
  }

  /** After the frame's last render call. */
  endFrame(): void {
    if (this.firstFrameMs !== null && !this.overlay) return;
    const now = performance.now();
    this.firstFrameMs ??= round(now - this.since);
    if (this.overlay && now - this.overlayAt >= OVERLAY_INTERVAL_MS) {
      this.overlayAt = now;
      this.overlay.textContent = this.describe(this.stats());
    }
  }

  stats(): PerfStats {
    const { render, memory, programs } = this.renderer.info;
    const sorted = Array.from(this.intervals.subarray(0, this.filled)).sort((a, b) => a - b);
    const sum = sorted.reduce((total, value) => total + value, 0);
    return {
      calls: render.calls,
      triangles: render.triangles,
      programs: programs?.length ?? 0,
      geometries: memory.geometries,
      textures: memory.textures,
      avgFrameMs: sorted.length ? round(sum / sorted.length) : null,
      p95FrameMs: sorted.length ? round(sorted[Math.ceil(sorted.length * 0.95) - 1]) : null,
      firstFrameMs: this.firstFrameMs,
    };
  }

  dispose(): void {
    this.overlay?.remove();
  }

  private describe(stats: PerfStats): string {
    const ms = (value: number | null) => (value === null ? "-" : value.toFixed(1));
    return [
      `draw call ${stats.calls}`,
      `삼각형 ${(stats.triangles / 1000).toFixed(1)}k`,
      `프로그램 ${stats.programs} · 지오메트리 ${stats.geometries} · 텍스처 ${stats.textures}`,
      `프레임 ${ms(stats.avgFrameMs)} ms · p95 ${ms(stats.p95FrameMs)} ms`,
      `첫 프레임 ${stats.firstFrameMs === null ? "-" : (stats.firstFrameMs / 1000).toFixed(1)} s`,
    ].join("\n");
  }
}
