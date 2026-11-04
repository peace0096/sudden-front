import * as THREE from "three";
import {
  Augment,
  HitZone,
  MapId,
  Team,
  type GameConfig,
  type MapBox,
  type MatchEnd,
  type MatchStart,
  type PlayerState,
  type RosterEntry,
  type Missile,
  type ScoreEntry,
  type Snapshot,
  type WeaponStats,
  WeaponKind,
} from "../gen/game_pb";
import { WEAPON_ORDER } from "../weapons";
import type { Connection } from "../net";
import { loadedArt, releaseGpu } from "./art/loader";
import { projectileArt } from "./art/manifest";
import { toLambert } from "./art/materials";
import { hitboxFrame, releaseWireGpu, type HitboxFrame } from "./art/view";
import { GameAudio } from "./audio";
import { Character, releaseCharacterGpu, setCharacterView } from "./character";
import { Effects } from "./effects";
import { Hud, type RadarBlip } from "./hud";
import { applyMove, CMD_DT, rayBox, rayWorld, type Vec2 } from "./movement";
import { PerfMeter, type PerfStats } from "./perf";
import { ViewModel } from "./viewmodel";
import { buildWorld, disposeWorld } from "./world";

const MOUSE_SENSITIVITY = 0.0022;
const MAX_PITCH = 1.45;
const INTERP_TICKS = 3;
const CMDS_PER_PACKET = 2;
const ENEMY_BLIP_SECONDS = 1.6;
const STEP_INTERVAL = 0.42;
const BASE_FOV = 72;
const SCOPE_FOV = 16;
const BLOOM_DECAY = 0.09;
// See BLOOM_HOLD_EXTRA in the server's systems.rs.
const BLOOM_HOLD_EXTRA = 0.1;
// Recoil moves the real aim: it is sent with each shot, not only drawn.
const RECOIL_RECOVERY = 0.25;
// Once the climb tops out the aim keeps bucking around up here instead of freezing.
const RECOIL_MAX = 0.14;
const RECOIL_SIDE_MAX = 0.06;
// See VEST_MAX_HP in the server's augments.rs.
const VEST_MAX_HP = 200;
// Reserved browser shortcuts like Ctrl+W only reach the page through the Keyboard
// Lock API, which needs fullscreen and a secure context.
const LOCKED_KEYS = ["KeyW", "KeyA", "KeyS", "KeyD", "KeyR"];
const RECOIL_PER_SHOT: Record<number, { up: number; side: number }> = {
  [WeaponKind.RIFLE]: { up: 0.012, side: 0.005 },
  [WeaponKind.SNIPER]: { up: 0.06, side: 0.01 },
  [WeaponKind.MACHINE_GUN]: { up: 0.008, side: 0.006 },
  [WeaponKind.SMG]: { up: 0.008, side: 0.005 },
  [WeaponKind.AKIMBO_SMG]: { up: 0.007, side: 0.006 },
  [WeaponKind.PISTOL]: { up: 0.022, side: 0.006 },
  [WeaponKind.PISTOL_PLUS]: { up: 0, side: 0.005 },
  [WeaponKind.MAGNUM]: { up: 0.12, side: 0 },
  [WeaponKind.KNIFE]: { up: 0, side: 0 },
  [WeaponKind.SHOTGUN]: { up: 0.05, side: 0.012 },
  [WeaponKind.MISSILE]: { up: 0.02, side: 0.006 },
  [WeaponKind.TASER]: { up: 0.01, side: 0 },
};
// See BURST_* in the server's weapons.rs.
const BURST_GAP = 0.07;
const BURST_RECOVERY = 0.2;
const BURST_ROUNDS = 3;
/** How far out from the camera the local player's tracers start, on the line through the gun's muzzle on screen. */
const LOCAL_TRACER_START = 1.2;

interface PendingCmd {
  seq: number;
  forward: boolean;
  back: boolean;
  left: boolean;
  right: boolean;
  yaw: number;
  /** Move speed of the weapon held for this command, so a replay matches it. */
  speed: number;
}

interface MissileView {
  object: THREE.Object3D;
  data: Missile;
  at: number;
  trail: number;
}

interface Sample {
  tick: number;
  x: number;
  z: number;
  yaw: number;
  pitch: number;
  crouching: boolean;
}

interface Remote {
  entry: RosterEntry;
  character: Character;
  samples: Sample[];
  state: PlayerState | null;
  shownX: number;
  shownZ: number;
  lastFired: number;
  /** The server's hitboxes around the player, under ?debug&hitbox. */
  hitbox: HitboxFrame | null;
}

function shortestAngle(from: number, to: number): number {
  let d = (to - from) % (Math.PI * 2);
  if (d > Math.PI) d -= Math.PI * 2;
  if (d < -Math.PI) d += Math.PI * 2;
  return d;
}

export class Game {
  // First, so the time to the first frame covers every field below.
  private readonly constructedAt = performance.now();
  private readonly renderer: THREE.WebGLRenderer;
  private readonly meter: PerfMeter;
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(BASE_FOV, 1, 0.08, 800);
  private readonly viewModel = new ViewModel();
  /** The map, the scene's "world" group (world.ts). */
  private readonly world: THREE.Group;
  private readonly effects: Effects;
  private readonly audio = new GameAudio();
  private readonly hud: Hud;
  private readonly config: GameConfig;
  private readonly boxes: MapBox[];
  private readonly myId: number;
  private readonly weapons: Map<WeaponKind, WeaponStats>;
  private weaponKind = WeaponKind.RIFLE;
  private selectedWeapon = WeaponKind.RIFLE;
  private primaryKind = WeaponKind.RIFLE;
  private sidearmKind = WeaponKind.PISTOL;
  private slotQueued = 0;
  private myAugment = Augment.UNSPECIFIED;
  private myActive: Augment[] = [];
  private frozen = false;
  private offerOpen = false;
  private offerKey = "";
  private offerCards: Augment[] = [];
  private localBurst = 0;
  private stunned = false;
  private readonly missiles = new Map<number, MissileView>();
  private readonly missileBody = new THREE.CylinderGeometry(0.11, 0.11, 0.65, 10);
  private readonly missileNose = new THREE.ConeGeometry(0.11, 0.2, 10);
  private readonly missileSkin = new THREE.MeshLambertMaterial({ color: 0xc8ccc0 });
  private readonly missileTip = new THREE.MeshLambertMaterial({ color: 0xc23a22 });
  // Reused for each shot's effects, which copy what they keep.
  private readonly shotFrom = new THREE.Vector3();
  private readonly shotEnd = new THREE.Vector3();
  private readonly shotMuzzle = new THREE.Vector3();
  private readonly shotDirection = new THREE.Vector3();
  private readonly missileVelocity = new THREE.Vector3();
  private readonly xrayMaterial = new THREE.MeshBasicMaterial({
    color: 0xff3a2a,
    transparent: true,
    opacity: 0.6,
    depthFunc: THREE.GreaterDepth,
    depthWrite: false,
    fog: false,
    // Pulled toward the camera so the model's own depth from the first pass never speckles it.
    polygonOffset: true,
    polygonOffsetFactor: -1,
    polygonOffsetUnits: -4,
  });
  /** ?debug&hitbox draws every remote player's server hitboxes. */
  private readonly showHitboxes = ((params) => params.has("debug") && params.has("hitbox"))(new URLSearchParams(location.search));
  // Until the server has processed this command, its state still shows the old weapon.
  private pendingSlotSeq = 0;
  private scopeOn = false;
  private menuOpen = false;
  private protectedNow = false;

  private roster = new Map<number, RosterEntry>();
  private remotes = new Map<number, Remote>();
  private myTeam = Team.RED;

  // Local player: predicted position, previous step for render smoothing, and
  // the leftover visual error after a server correction.
  private pos: Vec2 = { x: 0, z: 0 };
  private prev: Vec2 = { x: 0, z: 0 };
  private errX = 0;
  private errZ = 0;
  private yaw = 0;
  private pitch = 0;
  private alive = true;
  private crouching = false;
  private viewEye = 0;
  private hasFirstSnapshot = false;
  private pending: PendingCmd[] = [];
  private outgoing: Array<PendingCmd & { fire: boolean; reload: boolean; aim: boolean; pitch: number; renderTick: number; slot: number; crouch: boolean }> = [];
  private seq = 0;
  private accumulator = 0;

  private readonly keys = new Set<string>();
  private trigger = false;
  private reloadQueued = false;
  private locked = false;
  private matchOver = false;
  private boardLocked = false;

  private lastSnapshot: Snapshot | null = null;
  private snapshotAt = 0;
  private smoothTick = 0;
  private magazine = 30;
  private reserve = 120;
  private reloadRemaining = 0;
  private fireCooldown = 0;
  private bloom = 0;
  private bloomHold = 0;
  private recoil = 0;
  private recoilYaw = 0;
  private killerName: string | null = null;
  private stepTimer = 0;
  private movingNow = false;
  private pingTimer = 0;

  private lastFrame = performance.now();
  private frameHandle = 0;
  private disposed = false;

  private readonly onResize = () => this.resize();
  private readonly onKeyDown = (event: KeyboardEvent) => this.keyDown(event);
  private readonly onKeyUp = (event: KeyboardEvent) => this.keyUp(event);
  private readonly onMouseMove = (event: MouseEvent) => this.mouseMove(event);
  private readonly onMouseDown = (event: MouseEvent) => {
    if (event.button === 0 && this.locked) this.trigger = true;
    if (event.button === 2 && this.locked && this.stats().hasScope) this.scopeOn = !this.scopeOn;
  };
  // Ctrl+W can't be intercepted everywhere; don't lose a match to it.
  private readonly onBeforeUnload = (event: BeforeUnloadEvent) => event.preventDefault();
  private readonly onContextMenu = (event: MouseEvent) => event.preventDefault();
  private readonly onMouseUp = (event: MouseEvent) => {
    if (event.button === 0) this.trigger = false;
  };
  private readonly onLockChange = () => this.lockChanged();
  private readonly onBlur = () => {
    this.keys.clear();
    this.trigger = false;
  };

  constructor(
    private readonly gameRoot: HTMLElement,
    uiRoot: HTMLElement,
    start: MatchStart,
    private readonly connection: Connection,
  ) {
    this.config = start.config!;
    this.viewEye = this.config.eyeHeight;
    this.boxes = start.map!.boxes;
    this.myId = start.yourId;
    this.weapons = new Map(this.config.weapons.map((w) => [w.kind, w]));

    this.renderer = new THREE.WebGLRenderer({ antialias: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.shadowMap.enabled = true;
    this.renderer.shadowMap.type = THREE.PCFShadowMap;
    this.renderer.shadowMap.autoUpdate = false;
    this.renderer.shadowMap.needsUpdate = true;
    this.renderer.autoClear = false;
    this.renderer.toneMapping = THREE.ACESFilmicToneMapping;
    this.renderer.toneMappingExposure = 0.95;
    this.meter = new PerfMeter(this.renderer, uiRoot, this.constructedAt);
    gameRoot.append(this.renderer.domElement);

    this.world = buildWorld(this.scene, start.map!);
    this.effects = new Effects(this.scene, this.boxes, { indoor: start.map!.id === MapId.WAREHOUSE });
    this.scene.add(this.camera);

    this.hud = new Hud(
      uiRoot,
      this.boxes,
      () => this.connection.send({ case: "leaveRoom", value: {} }),
      () => this.requestLock(),
    );
    this.hud.setKillTarget(this.config.killsToWin);
    this.hud.renderBoard([], 0, 0, this.myId);

    this.onRoster(start.roster);
    this.hud.setWeaponName(this.stats().name);
    this.updateSlots();

    this.audio.resume();
    window.addEventListener("resize", this.onResize);
    window.addEventListener("keydown", this.onKeyDown);
    window.addEventListener("keyup", this.onKeyUp);
    window.addEventListener("mousemove", this.onMouseMove);
    window.addEventListener("mousedown", this.onMouseDown);
    window.addEventListener("mouseup", this.onMouseUp);
    window.addEventListener("contextmenu", this.onContextMenu);
    window.addEventListener("blur", this.onBlur);
    window.addEventListener("beforeunload", this.onBeforeUnload);
    document.addEventListener("pointerlockchange", this.onLockChange);
    this.resize();
    this.frameHandle = requestAnimationFrame(() => this.frame());
  }

  private updateSlots(): void {
    this.hud.setSlots(this.primaryKind, this.sidearmKind, this.weaponKind);
  }

  private reloadTime(): number {
    const base = this.stats().reloadTime;
    if (this.has(Augment.BORROW)) return 0;
    return base * (this.has(Augment.QUICK_RELOAD) ? 0.5 : 1) * (this.has(Augment.NO_RECOIL) ? 2 : 1);
  }

  private has(augment: Augment): boolean {
    return this.myActive.includes(augment);
  }

  private stats(): WeaponStats {
    return this.weapons.get(this.weaponKind)!;
  }

  /** Looking through the scope right now; it drops out for the bolt and reloads. */
  private scoped(): boolean {
    return (
      this.scopeOn &&
      this.stats().hasScope &&
      this.alive &&
      !this.matchOver &&
      !this.menuOpen &&
      this.reloadRemaining <= 0 &&
      this.fireCooldown <= 0
    );
  }

  private setWeapon(kind: WeaponKind): void {
    this.weaponKind = kind;
    this.scopeOn = false;
    this.bloom = 0;
    this.bloomHold = 0;
    this.fireCooldown = 0;
    this.viewModel.setWeapon(kind);
    this.localBurst = 0;
    this.hud.setWeaponName(this.stats().name);
    this.updateSlots();
  }

  // ------------------------------------------------------------ augment cards

  private syncOffer(snapshot: Snapshot): void {
    const open = snapshot.offered.length > 0 && snapshot.pickLeft > 0;
    if (open) {
      const key = `${snapshot.offered.join(",")}|${snapshot.refreshable.join(",")}`;
      if (key !== this.offerKey) {
        this.offerKey = key;
        this.offerCards = snapshot.offered;
        this.hud.showAugmentOffer(
          snapshot.offered,
          snapshot.refreshable,
          (augment) => this.pickAugment(augment),
          (slot) => this.connection.send({ case: "refreshAugments", value: { slot } }),
        );
      }
      this.hud.setOfferTimer(Math.ceil(snapshot.pickLeft));
    } else if (this.offerOpen) {
      this.hud.hideAugmentOffer();
    }
    if (open !== this.offerOpen) {
      this.offerOpen = open;
      this.hud.setPaused(!open && !this.locked && !this.matchOver && !this.menuOpen);
    }
    const waiting = snapshot.pickLeft > 0
      ? `다른 플레이어 선택 대기 ${Math.ceil(snapshot.pickLeft)}`
      : snapshot.freezeLeft > 0
        ? `전투 시작까지 ${Math.ceil(snapshot.freezeLeft)}`
        : "";
    this.hud.setCountdown(open ? "" : waiting);
  }

  private pickAugment(augment: Augment): void {
    this.connection.send({ case: "pickAugment", value: { augment } });
    // The click is the gesture that can capture the mouse for the coming fight.
    this.requestLock();
  }

  // ------------------------------------------------------------ weapon menu

  private openWeaponMenu(): void {
    if (this.matchOver) return;
    this.menuOpen = true;
    this.renderWeaponMenu();
    this.hud.setPaused(false);
    // The cursor is freed so cards can be clicked.
    if (document.pointerLockElement) document.exitPointerLock();
  }

  private renderWeaponMenu(): void {
    const immediate = this.alive && this.protectedNow;
    this.hud.showWeaponMenu(
      this.config.weapons,
      this.selectedWeapon,
      (kind) => this.pickWeapon(kind),
      immediate ? "재투입 보호 중이므로 즉시 교체됩니다." : "다음 재투입 때 적용됩니다.",
    );
  }

  private pickWeapon(kind: WeaponKind): void {
    this.selectedWeapon = kind;
    this.connection.send({ case: "selectWeapon", value: { weapon: kind } });
    this.closeWeaponMenu();
  }

  private closeWeaponMenu(): void {
    this.menuOpen = false;
    this.hud.hideWeaponMenu();
    if (this.matchOver) return;
    this.requestLock();
    this.hud.setPaused(!this.locked);
  }

  // ------------------------------------------------------------ input

  private requestLock(): void {
    this.audio.resume();
    // requestPointerLock rejects when it's retried too soon after Esc.
    const lockPointer = () => {
      const result = this.renderer.domElement.requestPointerLock() as unknown as Promise<void> | undefined;
      result?.catch?.(() => undefined);
    };
    const root = document.documentElement;
    if (document.fullscreenElement || !root.requestFullscreen) {
      lockPointer();
      return;
    }
    root
      .requestFullscreen()
      .then(() => this.keyboardLock()?.lock(LOCKED_KEYS))
      .catch(() => undefined)
      .finally(lockPointer);
  }

  private keyboardLock(): { lock(keys: string[]): Promise<void>; unlock(): void } | undefined {
    return (navigator as unknown as { keyboard?: { lock(keys: string[]): Promise<void>; unlock(): void } }).keyboard;
  }

  private eyeHeight(): number {
    return this.crouching ? this.config.crouchEyeHeight : this.config.eyeHeight;
  }

  private lockChanged(): void {
    this.locked = document.pointerLockElement === this.renderer.domElement;
    if (!this.locked) {
      this.keys.clear();
      this.trigger = false;
    }
    this.hud.setPaused(!this.locked && !this.matchOver && !this.menuOpen && !this.offerOpen);
  }

  private keyDown(event: KeyboardEvent): void {
    if (event.code === "Tab") {
      event.preventDefault();
      if (!this.boardLocked) {
        this.refreshBoard();
        this.hud.showBoard(true);
      }
      return;
    }
    if (this.offerOpen) {
      const digit = /^Digit([1-3])$/.exec(event.code);
      if (digit && !event.repeat && this.offerCards[Number(digit[1]) - 1]) this.pickAugment(this.offerCards[Number(digit[1]) - 1]);
      return;
    }
    if (event.code === "KeyB" && !event.repeat) {
      if (this.menuOpen) this.closeWeaponMenu();
      else this.openWeaponMenu();
      return;
    }
    if (this.menuOpen) {
      if (event.code === "Escape") this.closeWeaponMenu();
      const digit = /^Digit([1-4])$/.exec(event.code);
      if (digit) this.pickWeapon(WEAPON_ORDER[Number(digit[1]) - 1]);
      return;
    }
    if (!this.locked) return;
    // Ctrl is crouch here, so Ctrl+S and friends must not reach the browser.
    if (event.ctrlKey) event.preventDefault();
    this.keys.add(event.code);
    if (event.code === "KeyR" && !event.repeat) this.reloadQueued = true;
    const slot = /^Digit([1-3])$/.exec(event.code);
    if (slot && !event.repeat) this.slotQueued = Number(slot[1]);
  }

  private keyUp(event: KeyboardEvent): void {
    if (event.code === "Tab") {
      event.preventDefault();
      if (!this.boardLocked) this.hud.showBoard(false);
      return;
    }
    this.keys.delete(event.code);
  }

  private mouseMove(event: MouseEvent): void {
    if (!this.locked) return;
    // Slower while zoomed so the same hand movement covers the same screen distance.
    const sensitivity = MOUSE_SENSITIVITY * (this.camera.fov / BASE_FOV);
    this.yaw -= event.movementX * sensitivity;
    this.pitch = Math.max(-MAX_PITCH, Math.min(MAX_PITCH, this.pitch - event.movementY * sensitivity));
  }

  // ------------------------------------------------------------ network

  onRoster(roster: RosterEntry[]): void {
    this.roster = new Map(roster.map((entry) => [entry.id, entry]));
    this.myTeam = this.roster.get(this.myId)?.team ?? this.myTeam;
    this.viewModel.setTeam(this.myTeam);

    for (const [id, remote] of this.remotes) {
      if (!this.roster.has(id)) {
        remote.character.dispose();
        remote.hitbox?.dispose();
        this.remotes.delete(id);
      }
    }
    for (const entry of roster) {
      if (entry.id === this.myId || this.remotes.has(entry.id)) continue;
      const character = new Character(entry.name, entry.team);
      if (entry.team !== this.myTeam) character.enableXray();
      this.scene.add(character.root);
      const hitbox = this.showHitboxes ? hitboxFrame() : null;
      if (hitbox) this.scene.add(hitbox.group);
      this.remotes.set(entry.id, {
        entry,
        character,
        samples: [],
        state: null,
        shownX: 0,
        shownZ: 0,
        lastFired: -100,
        hitbox,
      });
    }
  }

  onPong(clientTime: number): void {
    this.hud.setPing(performance.now() - clientTime);
  }

  onSnapshot(snapshot: Snapshot): void {
    this.lastSnapshot = snapshot;
    this.snapshotAt = performance.now();
    if (!this.hasFirstSnapshot) this.smoothTick = snapshot.tick;
    this.frozen = snapshot.freezeLeft > 0;
    this.syncOffer(snapshot);
    this.syncMissiles(snapshot.missiles);

    // Ammo fields describe the weapon in hand, so skip them until a switch lands.
    if (snapshot.lastInputSeq >= this.pendingSlotSeq) {
      this.magazine = snapshot.magazine;
      this.reserve = snapshot.reserve;
      this.reloadRemaining = snapshot.reloadRemaining;
    }
    if (snapshot.primaryWeapon && snapshot.primaryWeapon !== this.primaryKind) {
      this.primaryKind = snapshot.primaryWeapon;
      this.updateSlots();
    }
    if (snapshot.sidearmWeapon && snapshot.sidearmWeapon !== this.sidearmKind) {
      this.sidearmKind = snapshot.sidearmWeapon;
      this.updateSlots();
    }
    if (snapshot.selectedWeapon) this.selectedWeapon = snapshot.selectedWeapon;

    for (const player of snapshot.players) {
      if (player.id === this.myId) {
        this.reconcile(player, snapshot.lastInputSeq);
        continue;
      }
      const remote = this.remotes.get(player.id);
      if (!remote) continue;
      remote.state = player;
      remote.character.setWeapon(player.weapon);
      remote.character.setLook(player.headScale, player.shieldHp > 0 && player.weapon === WeaponKind.PISTOL, player.stunLeft > 0);
      remote.samples.push({ tick: snapshot.tick, x: player.x, z: player.z, yaw: player.yaw, pitch: player.pitch, crouching: player.crouching });
      if (remote.samples.length > 40) remote.samples.shift();
    }

    this.hud.setScore(snapshot.redScore, snapshot.blueScore);
    this.hud.setClock(snapshot.clock);
    this.hud.setAmmo(
      this.magazine,
      this.reserve,
      this.reloadRemaining,
      this.reloadTime(),
      this.stats().meleeRange > 0,
      this.has(Augment.INFINITE_AMMO),
      Math.floor(this.stats().magazine / 6),
    );

    for (const { event } of snapshot.events) {
      switch (event.case) {
        case "shot":
          this.onShot(event.value);
          break;
        case "damage":
          this.onDamage(event.value.attackerId, event.value.victimId, event.value.zone);
          break;
        case "kill":
          this.onKill(event.value.killerId, event.value.victimId, event.value.headshot, event.value.weapon);
          break;
        case "explosion": {
          const center = new THREE.Vector3(event.value.x, event.value.y, event.value.z);
          this.effects.explosion(center);
          const { distance, pan } = this.relative(center.x, center.z);
          this.audio.explosion(distance, pan);
          break;
        }
        case "reload":
          if (event.value.playerId === this.myId) {
            if (this.reloadTime() > 0) this.audio.reload(this.reloadTime());
            this.scopeOn = false;
          }
          break;
        default:
          break;
      }
    }
    this.hasFirstSnapshot = true;
  }

  private reconcile(me: PlayerState, ackSeq: number): void {
    const wasAlive = this.alive;
    this.alive = me.alive;
    this.protectedNow = me.protected;
    this.stunned = me.alive && me.stunLeft > 0;
    if (this.stunned) this.crouching = me.crouching;
    this.hud.setStun(this.stunned ? me.stunLeft : 0);
    if (ackSeq >= this.pendingSlotSeq && me.weapon !== this.weaponKind) this.setWeapon(me.weapon);
    if (!me.alive) this.scopeOn = false;
    if (me.augment !== this.myAugment) {
      this.myAugment = me.augment;
      this.myActive = me.active;
      this.hud.setAugmentBadge(me.augment, me.active);
      this.updateSlots();
    }
    this.hud.setHealth(me.hp, this.has(Augment.VEST) ? VEST_MAX_HP : this.config.maxHp);
    this.hud.setShield(me.shieldHp);
    this.viewModel.setShield(me.alive && me.shieldHp > 0 && this.weaponKind === WeaponKind.PISTOL);

    if (me.alive) {
      this.hud.hideDeath();
    } else {
      this.hud.showDeath(this.killerName, me.respawnIn);
    }

    // First snapshot or a respawn places us; the server also chose our facing.
    if (!this.hasFirstSnapshot || (!wasAlive && me.alive)) {
      this.yaw = me.yaw;
      this.pitch = me.pitch;
      this.recoil = 0;
      this.recoilYaw = 0;
      this.pos = { x: me.x, z: me.z };
      this.prev = { ...this.pos };
      this.errX = this.errZ = 0;
      this.pending = this.pending.filter((cmd) => cmd.seq > ackSeq);
      this.killerName = null;
      return;
    }

    // Rewind to the authoritative state and replay what it hasn't processed.
    this.pending = this.pending.filter((cmd) => cmd.seq > ackSeq);
    const before = { ...this.pos };
    const replay: Vec2 = { x: me.x, z: me.z };
    if (me.alive) {
      for (const cmd of this.pending) applyMove(replay, cmd, this.boxes, this.config, cmd.speed);
    }
    const dx = before.x - replay.x;
    const dz = before.z - replay.z;
    if (Math.hypot(dx, dz) > 2) {
      this.errX = this.errZ = 0;
    } else {
      this.errX += dx;
      this.errZ += dz;
    }
    this.pos = replay;
    this.prev = {
      x: this.prev.x - dx,
      z: this.prev.z - dz,
    };
  }

  private playerPosition(id: number): THREE.Vector3 | null {
    if (id === this.myId) return new THREE.Vector3(this.pos.x, this.config.eyeHeight, this.pos.z);
    const remote = this.remotes.get(id);
    if (!remote) return null;
    return new THREE.Vector3(remote.shownX, this.config.eyeHeight, remote.shownZ);
  }

  private onShot(shot: {
    shooterId: number;
    fromX: number;
    fromY: number;
    fromZ: number;
    toX: number;
    toY: number;
    toZ: number;
    hitPlayerId: number;
    hitWorld: boolean;
    melee: boolean;
    pellet: boolean;
    explosive: boolean;
  }): void {
    const from = this.shotFrom.set(shot.fromX, shot.fromY, shot.fromZ);
    const end = this.shotEnd.set(shot.toX, shot.toY, shot.toZ);
    // The server's world ray leaves the ground out: a shot into it ends under it, so it is stopped
    // where it crosses the ground, and kicks up dust there.
    const grounded = !shot.explosive && !shot.hitWorld && !shot.hitPlayerId && end.y < 0 && from.y > 0;
    if (grounded) end.lerpVectors(from, end, from.y / (from.y - end.y));
    if (shot.explosive) {
      // A launch: the missile itself comes with the snapshots.
    } else if (shot.hitWorld || grounded) this.effects.impact(end, from, shot.melee);
    else if (shot.hitPlayerId) this.effects.bloodPuff(end, from);

    // A shotgun's other pellets share the first one's flash and sound.
    if (shot.pellet) {
      const remote = this.remotes.get(shot.shooterId);
      if (remote && shot.shooterId !== this.myId) {
        this.effects.tracer(remote.character.muzzleWorldPosition(this.shotMuzzle), end);
      }
      return;
    }

    if (shot.shooterId === this.myId) return; // Already shown when fired locally.
    const remote = this.remotes.get(shot.shooterId);
    if (!remote) return;
    remote.lastFired = performance.now() / 1000;
    remote.character.kick();
    const muzzle = remote.character.muzzleWorldPosition(this.shotMuzzle);
    if (!shot.melee) {
      // A launch reports no end (it is the shooter's eye), so its flash has no flame along the barrel.
      this.effects.muzzleFlash(muzzle, shot.explosive ? undefined : this.shotDirection.subVectors(end, muzzle));
      if (!shot.explosive) this.effects.tracer(muzzle, end);
    }
    const { distance, pan } = this.relative(muzzle.x, muzzle.z);
    this.audio.gunshot(remote.state?.weapon ?? WeaponKind.RIFLE, distance, pan);
  }

  private relative(x: number, z: number): { distance: number; pan: number; angle: number } {
    const dx = x - this.pos.x;
    const dz = z - this.pos.z;
    const distance = Math.hypot(dx, dz);
    const rightness = dx * Math.cos(this.yaw) - dz * Math.sin(this.yaw);
    const forwardness = -dx * Math.sin(this.yaw) - dz * Math.cos(this.yaw);
    return {
      distance,
      pan: distance > 0.01 ? (rightness / distance) * 0.8 : 0,
      angle: Math.atan2(rightness, forwardness),
    };
  }

  private onDamage(attackerId: number, victimId: number, zone: HitZone): void {
    if (attackerId === this.myId) {
      this.audio.hitConfirm(zone === HitZone.HEAD);
      this.hud.showHit(zone === HitZone.HEAD, false);
    }
    if (victimId === this.myId) {
      this.audio.hurt();
      const attacker = this.playerPosition(attackerId);
      if (attacker) this.hud.showHurt(this.relative(attacker.x, attacker.z).angle);
    }
  }

  private onKill(killerId: number, victimId: number, headshot: boolean, weapon: WeaponKind): void {
    const killer = this.roster.get(killerId);
    const victim = this.roster.get(victimId);
    if (killer && victim) {
      this.hud.addKill({
        killer: killer.name,
        killerTeam: killer.team,
        victim: victim.name,
        victimTeam: victim.team,
        headshot,
        weapon,
      });
    }
    if (killerId === this.myId) this.hud.showHit(headshot, true);
    if (victimId === this.myId) {
      this.killerName = killer?.name ?? null;
      this.trigger = false;
    }
  }

  onMatchEnd(end: MatchEnd): void {
    this.matchOver = true;
    this.boardLocked = true;
    this.menuOpen = false;
    this.hud.hideWeaponMenu();
    document.exitPointerLock();
    this.hud.setPaused(false);
    this.hud.hideDeath();
    const victory = end.winner === this.myTeam;
    const winnerLabel = end.winner === Team.RED ? "RED TEAM" : "BLUE TEAM";
    this.hud.renderBoard(end.scoreboard, end.redScore, end.blueScore, this.myId, {
      title: victory ? "VICTORY" : "DEFEAT",
      subtitle: `${winnerLabel} 승리 · ${Math.round(end.returnIn)}초 후 대기실로 돌아갑니다`,
      victory,
    });
    this.hud.showBoard(true);
  }

  private refreshBoard(): void {
    const snapshot = this.lastSnapshot;
    if (!snapshot) return;
    const entries = snapshot.players.flatMap((player): ScoreEntry[] => {
      const entry = this.roster.get(player.id);
      if (!entry) return [];
      return [
        {
          id: player.id,
          name: entry.name,
          team: entry.team,
          kills: player.kills,
          deaths: player.deaths,
          isBot: entry.isBot,
        } as ScoreEntry,
      ];
    });
    this.hud.renderBoard(entries, snapshot.redScore, snapshot.blueScore, this.myId);
  }

  // ------------------------------------------------------------ simulation

  private estimatedServerTick(): number {
    if (!this.lastSnapshot) return 0;
    const elapsed = (performance.now() - this.snapshotAt) / 1000;
    return this.lastSnapshot.tick + elapsed * this.config.tickRate;
  }

  private stepInput(): void {
    this.prev = { ...this.pos };
    const movementAllowed = this.locked && this.alive && !this.matchOver && !this.frozen && !this.stunned;
    const held = (code: string) => movementAllowed && this.keys.has(code);

    // Switch before moving: this command is simulated at the new weapon's speed.
    const slot = this.alive && !this.frozen && !this.stunned ? this.slotQueued : 0;
    this.slotQueued = 0;
    if (slot) {
      const kind = slot === 1 ? this.primaryKind : slot === 2 ? this.sidearmKind : WeaponKind.KNIFE;
      if (kind !== this.weaponKind) {
        const carried = this.fireCooldown;
        this.setWeapon(kind);
        this.fireCooldown = Math.max(carried, this.config.drawTime);
        this.reloadRemaining = 0;
        this.pendingSlotSeq = this.seq + 1;
      }
    }

    // The server holds a paralysed player's stance.
    if (!this.stunned) this.crouching = held("ControlLeft") || held("ControlRight") || held("KeyC");
    const cmd: PendingCmd = {
      seq: ++this.seq,
      forward: held("KeyW"),
      back: held("KeyS"),
      left: held("KeyA"),
      right: held("KeyD"),
      yaw: this.aimYaw(),
      speed: this.stats().moveSpeed * (this.crouching ? this.config.crouchSpeedFactor : 1),
    };
    if (this.alive) applyMove(this.pos, cmd, this.boxes, this.config, cmd.speed);
    this.movingNow = cmd.forward || cmd.back || cmd.left || cmd.right;
    this.pending.push(cmd);
    if (this.pending.length > 240) this.pending.shift();

    // Read before firing: a shot leaves from where the crosshair was, and its
    // recoil only moves the ones after it.
    const aimPitch = this.aimPitch();
    // A burst finishes after the button is released.
    const firing = (this.trigger || this.localBurst > 0) && movementAllowed;
    if (firing) this.tryLocalFire();

    // Remote players are displayed in the past; the server rewinds to the same
    // moment. History samples are labelled one tick before the snapshot.
    const renderTick = Math.max(0, Math.round(this.smoothTick - INTERP_TICKS) - 1);
    this.outgoing.push({ ...cmd, fire: firing, reload: this.reloadQueued, aim: this.scoped(), pitch: aimPitch, renderTick, slot, crouch: this.crouching });
    this.reloadQueued = false;

    if (this.outgoing.length >= CMDS_PER_PACKET) {
      this.connection.send({
        case: "input",
        value: {
          cmds: this.outgoing.map((c) => ({
            seq: c.seq,
            forward: c.forward,
            back: c.back,
            left: c.left,
            right: c.right,
            fire: c.fire,
            reload: c.reload,
            aim: c.aim,
            slot: c.slot,
            crouch: c.crouch,
            yaw: c.yaw,
            pitch: c.pitch,
            renderTick: c.renderTick,
          })),
        },
      });
      this.outgoing = [];
    }
  }

  private tryLocalFire(): void {
    if (this.fireCooldown > 0) return;
    const stats = this.stats();
    if (this.reloadRemaining > 0 && !(stats.shellReload && this.magazine > 0)) return;
    if (stats.meleeRange > 0) {
      this.fireCooldown = stats.fireInterval;
      this.viewModel.fire();
      this.audio.gunshot(this.weaponKind, 0, 0);
      return;
    }
    if (this.magazine === 0) {
      this.localBurst = 0;
      // With spare ammo the server is already starting an automatic reload.
      if (this.reserve === 0 && !this.has(Augment.INFINITE_AMMO)) {
        this.audio.dryFire();
        this.fireCooldown = 0.25;
      }
      return;
    }
    this.reloadRemaining = 0;
    if (this.weaponKind === WeaponKind.PISTOL_PLUS) {
      this.localBurst = this.localBurst > 0 ? this.localBurst - 1 : BURST_ROUNDS - 1;
      this.fireCooldown = this.localBurst > 0 ? BURST_GAP : BURST_RECOVERY;
    } else {
      this.fireCooldown = stats.fireInterval;
    }
    this.viewModel.fire();
    this.audio.gunshot(this.weaponKind, 0, 0);
    if (stats.hasScope) this.audio.bolt();
    if (stats.shellReload) this.audio.pump();
    const noRecoil = this.has(Augment.NO_RECOIL);
    if (!noRecoil) {
      this.bloom = Math.min(this.bloom + stats.bloomPerShot, stats.bloomMax);
      this.bloomHold = stats.fireInterval + BLOOM_HOLD_EXTRA;
    }

    // Local tracer from the gun towards where the crosshair points, stopped by the map or the ground.
    const eye: [number, number, number] = [this.pos.x, this.eyeHeight(), this.pos.z];
    const dir = this.aimDirection();
    let t = rayWorld(this.boxes, eye, dir, 200) ?? 200;
    if (dir[1] < 0) t = Math.min(t, eye[1] / -dir[1]);
    const end = new THREE.Vector3(eye[0] + dir[0] * t, eye[1] + dir[1] * t, eye[2] + dir[2] * t);
    // It starts where the gun's muzzle is on screen, a little way out along that line of sight.
    const from = this.viewModel.muzzleNdc(this.shotFrom).setZ(0.5).unproject(this.camera).sub(this.camera.position);
    from.setLength(LOCAL_TRACER_START).add(this.camera.position);
    if (stats.splashRadius === 0) this.effects.tracer(from, end);
    // The server's own pellets land where they land; these only show the spread.
    for (let i = 1; i < stats.pellets; i++) {
      const jitter = new THREE.Vector3(Math.random() - 0.5, Math.random() - 0.5, Math.random() - 0.5).multiplyScalar(t * stats.spreadStill * 2);
      this.effects.tracer(from, end.clone().add(jitter));
    }
    if (noRecoil) return;
    this.kickAim();
  }

  /** Every shot throws the aim up and sideways, however long the trigger is held. */
  private kickAim(): void {
    const kick = RECOIL_PER_SHOT[this.weaponKind];
    if (!kick) return;
    const cap = Math.max(RECOIL_MAX, kick.up);
    this.recoil += kick.up * (0.8 + Math.random() * 0.4);
    if (this.recoil > cap) this.recoil = cap - kick.up * (0.5 + Math.random() * 2);
    this.recoilYaw += (Math.random() - 0.5) * 2 * kick.side;
    this.recoilYaw = Math.max(-RECOIL_SIDE_MAX, Math.min(RECOIL_SIDE_MAX, this.recoilYaw));
  }

  private settleRecoil(dt: number): void {
    if (this.recoil <= 0) {
      this.recoilYaw = 0;
      return;
    }
    const next = Math.max(0, this.recoil - dt * RECOIL_RECOVERY);
    this.recoilYaw *= next / this.recoil;
    this.recoil = next;
  }

  private aimYaw(): number {
    return this.yaw + this.recoilYaw;
  }

  private syncMissiles(list: Missile[]): void {
    const live = new Set(list.map((m) => m.id));
    for (const [id, view] of this.missiles) {
      if (!live.has(id)) {
        view.object.removeFromParent();
        this.missiles.delete(id);
      }
    }
    const now = performance.now() / 1000;
    for (const data of list) {
      let view = this.missiles.get(data.id);
      if (!view) {
        view = { object: this.buildMissile(), data, at: now, trail: 0 };
        this.scene.add(view.object);
        this.missiles.set(data.id, view);
      }
      view.data = data;
      view.at = now;
    }
  }

  /** A warhead pointing along +Z, which is what lookAt aims: the art's model, or one built here. */
  private buildMissile(): THREE.Object3D {
    const art = projectileArt(WeaponKind.MISSILE);
    const gltf = art && loadedArt()?.models.get(art.file);
    if (art && gltf) {
      const model = gltf.scene.clone();
      toLambert(model);
      if (!art.placement) return model;
      const holder = new THREE.Group();
      model.position.fromArray(art.placement.position ?? [0, 0, 0]);
      model.rotation.fromArray([...(art.placement.rotation ?? [0, 0, 0]), "XYZ"] as [number, number, number, THREE.EulerOrder]);
      model.scale.setScalar(art.placement.scale ?? 1);
      holder.add(model);
      return holder;
    }
    const group = new THREE.Group();
    const body = new THREE.Mesh(this.missileBody, this.missileSkin);
    body.rotation.x = Math.PI / 2;
    const nose = new THREE.Mesh(this.missileNose, this.missileTip);
    nose.rotation.x = Math.PI / 2;
    nose.position.z = 0.42;
    group.add(body, nose);
    return group;
  }

  private updateMissiles(dt: number, now: number): void {
    for (const view of this.missiles.values()) {
      const { data } = view;
      const flown = now - view.at;
      const x = data.x + data.vx * flown;
      const y = data.y + data.vy * flown;
      const z = data.z + data.vz * flown;
      view.object.position.set(x, y, z);
      view.object.lookAt(x + data.vx, y + data.vy, z + data.vz);
      // A puff every 25 ms of flight, each where the missile was when it was due, so the trail
      // keeps its spacing at any frame rate.
      view.trail -= dt;
      const velocity = this.missileVelocity.set(data.vx, data.vy, data.vz);
      for (; view.trail <= 0; view.trail += 0.025) {
        this.effects.missileTrail(this.shotEnd.copy(view.object.position).addScaledVector(velocity, view.trail), velocity);
      }
    }
  }

  /** Where the crosshair really points: the mouse's aim lifted by recoil. */
  private aimPitch(): number {
    return Math.min(MAX_PITCH, this.pitch + this.recoil);
  }

  private aimDirection(): [number, number, number] {
    const pitch = this.aimPitch();
    const cp = Math.cos(pitch);
    const yaw = this.aimYaw();
    return [-Math.sin(yaw) * cp, Math.sin(pitch), -Math.cos(yaw) * cp];
  }

  private sampleRemote(remote: Remote, renderTick: number): Sample | null {
    const samples = remote.samples;
    if (samples.length === 0) return null;
    for (let i = samples.length - 1; i >= 0; i--) {
      if (samples[i].tick <= renderTick) {
        const a = samples[i];
        const b = samples[i + 1];
        if (!b) return a;
        // Respawn teleports must not slide across the map.
        if (Math.hypot(b.x - a.x, b.z - a.z) > 3) return a;
        const f = (renderTick - a.tick) / (b.tick - a.tick);
        return {
          tick: renderTick,
          x: a.x + (b.x - a.x) * f,
          z: a.z + (b.z - a.z) * f,
          yaw: a.yaw + shortestAngle(a.yaw, b.yaw) * f,
          pitch: a.pitch + (b.pitch - a.pitch) * f,
          crouching: a.crouching,
        };
      }
    }
    return samples[0];
  }

  /** Whether a straight line from our eye reaches a player's head without crossing the map. */
  private canSeeHead(x: number, z: number, crouching: boolean): boolean {
    const eye: [number, number, number] = [this.pos.x, this.eyeHeight(), this.pos.z];
    const dx = x - eye[0];
    const dy = (crouching ? this.config.crouchEyeHeight : this.config.eyeHeight) - eye[1];
    const dz = z - eye[2];
    const distance = Math.hypot(dx, dy, dz);
    if (distance < 1e-3) return true;
    return rayWorld(this.boxes, eye, [dx / distance, dy / distance, dz / distance], distance) === null;
  }

  /** Enemy under the crosshair, whose name is then shown. */
  private aimedEnemy(): number | null {
    const eye: [number, number, number] = [this.pos.x, this.eyeHeight(), this.pos.z];
    const dir = this.aimDirection();
    const wall = rayWorld(this.boxes, eye, dir, 200) ?? 200;
    let best: number | null = null;
    let bestT = wall;
    for (const [id, remote] of this.remotes) {
      if (remote.entry.team === this.myTeam || !remote.state?.alive) continue;
      const pad = 0.42;
      const t = rayBox(
        eye,
        dir,
        [remote.shownX - pad, 0, remote.shownZ - pad],
        [remote.shownX + pad, remote.state.crouching ? 1.35 : 1.85, remote.shownZ + pad],
      );
      if (t !== null && t < bestT) {
        bestT = t;
        best = id;
      }
    }
    return best;
  }

  // ------------------------------------------------------------ rendering

  private resize(): void {
    const width = window.innerWidth;
    const height = window.innerHeight;
    this.renderer.setSize(width, height);
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
    this.viewModel.setAspect(width / height);
  }

  private frame(): void {
    if (this.disposed) return;
    this.frameHandle = requestAnimationFrame(() => this.frame());
    const now = performance.now();
    this.meter.beginFrame(now);
    const dt = Math.min((now - this.lastFrame) / 1000, 0.1);
    this.lastFrame = now;
    const time = now / 1000;

    if (this.hasFirstSnapshot && !this.matchOver) {
      // Chase the estimated server clock smoothly so jitter doesn't cause judder.
      const estimate = this.estimatedServerTick();
      this.smoothTick += dt * this.config.tickRate;
      this.smoothTick += (estimate - this.smoothTick) * Math.min(1, dt * 4);

      this.accumulator += dt;
      while (this.accumulator >= CMD_DT) {
        this.fireCooldown = Math.max(0, this.fireCooldown - CMD_DT);
        this.stepInput();
        this.accumulator -= CMD_DT;
      }
    }
    if (this.bloomHold > 0) this.bloomHold -= dt;
    else this.bloom = Math.max(0, this.bloom - dt * BLOOM_DECAY);
    // Recoil settles only between bursts, so a held trigger keeps climbing.
    if (this.fireCooldown <= 0 && !this.trigger) this.settleRecoil(dt);
    if (this.reloadRemaining > 0) this.reloadRemaining = Math.max(0, this.reloadRemaining - dt);

    const decay = Math.exp(-dt * 12);
    this.errX *= decay;
    this.errZ *= decay;
    const alpha = this.accumulator / CMD_DT;
    const x = this.prev.x + (this.pos.x - this.prev.x) * alpha + this.errX;
    const z = this.prev.z + (this.pos.z - this.prev.z) * alpha + this.errZ;

    // The stance changes at once in the simulation; only the view eases into it.
    this.viewEye += (this.eyeHeight() - this.viewEye) * Math.min(1, dt * 14);
    const eyeY = this.alive ? this.viewEye : 0.35;
    this.camera.position.set(x, eyeY, z);
    this.camera.rotation.order = "YXZ";
    this.camera.rotation.set(this.aimPitch(), this.aimYaw(), this.alive ? 0 : 0.5);
    const scoped = this.scoped();
    const targetFov = scoped ? SCOPE_FOV : BASE_FOV;
    if (this.camera.fov !== targetFov) {
      // Snaps in like a real scope view rather than a slow zoom.
      this.camera.fov = targetFov;
      this.camera.updateProjectionMatrix();
    }
    // The view is final before the remote players are updated, so those out of it skip their pose.
    setCharacterView(this.camera);

    const renderTick = this.smoothTick - INTERP_TICKS;
    const aimed = this.aimedEnemy();
    const blips: RadarBlip[] = [];
    for (const [id, remote] of this.remotes) {
      const sample = this.sampleRemote(remote, renderTick);
      const state = remote.state;
      if (remote.hitbox) remote.hitbox.group.visible = !!sample && !!state?.alive;
      if (!sample || !state) {
        remote.character.root.visible = false;
        continue;
      }
      if (remote.hitbox) {
        remote.hitbox.group.position.set(sample.x, 0, sample.z);
        const shielded = state.shieldHp > 0 && state.weapon === WeaponKind.PISTOL;
        remote.hitbox.pose(sample.crouching, state.headScale, shielded ? sample.yaw : null);
      }
      remote.shownX = sample.x;
      remote.shownZ = sample.z;
      remote.character.update(dt, sample.x, sample.z, sample.yaw, sample.pitch, state.moving, state.alive, state.protected, time, sample.crouching);
      const ally = remote.entry.team === this.myTeam;
      remote.character.setNameVisible((ally || id === aimed) && this.canSeeHead(sample.x, sample.z, sample.crouching));
      if (state.alive) {
        if (ally) blips.push({ x: sample.x, z: sample.z, enemy: false });
        else if (time - remote.lastFired < ENEMY_BLIP_SECONDS) blips.push({ x: sample.x, z: sample.z, enemy: true });
      }
    }

    // Footsteps and head bob are local-only polish.
    if (this.movingNow && this.alive) {
      this.stepTimer -= dt;
      if (this.stepTimer <= 0) {
        this.audio.footstep();
        this.stepTimer = this.crouching ? STEP_INTERVAL * 1.6 : STEP_INTERVAL;
      }
    }

    const stats = this.stats();
    const reloadFraction = this.reloadRemaining > 0 && this.reloadTime() > 0 ? 1 - this.reloadRemaining / this.reloadTime() : 0;
    const boltFraction = stats.hasScope || stats.shellReload ? this.fireCooldown / stats.fireInterval : 0;
    this.viewModel.update(dt, this.movingNow, reloadFraction, boltFraction, this.alive);

    this.hud.setScope(scoped);
    this.updateMissiles(dt, now / 1000);
    this.effects.update(dt);
    this.hud.update(dt);
    // The crosshair opens to the real cone, projected to pixels.
    const stance = this.crouching ? this.config.crouchSpreadFactor : 1;
    const steady = this.has(Augment.NO_RECOIL) && stats.pellets === 0;
    const cone = steady ? 0 : (stats.spreadStill + (this.movingNow ? stats.spreadMoving : 0) + this.bloom) * stance;
    const conePx = (Math.tan(cone) / Math.tan(THREE.MathUtils.degToRad(BASE_FOV / 2))) * (window.innerHeight / 2);
    this.hud.setCrosshair(6 + conePx, this.alive && !this.matchOver && !scoped);
    this.hud.drawRadar(x, z, this.yaw, blips);

    this.pingTimer -= dt;
    if (this.pingTimer <= 0) {
      this.pingTimer = 2;
      this.connection.send({ case: "ping", value: { clientTime: performance.now() } });
    }

    this.renderer.clear();
    this.renderer.render(this.scene, this.camera);
    if (this.has(Augment.WALLHACK)) this.renderXray();
    if (!scoped) {
      this.renderer.clearDepth();
      this.renderer.render(this.viewModel.scene, this.viewModel.camera);
    }
    this.meter.endFrame();
  }

  /** Rendering cost and frame times, for e2e/perf.mjs and the ?debug&perf overlay. */
  perf(): PerfStats {
    return this.meter.stats();
  }

  /** Wallhack: enemy models again, only where the world hides them. */
  private renderXray(): void {
    this.camera.layers.set(1);
    this.scene.overrideMaterial = this.xrayMaterial;
    this.renderer.render(this.scene, this.camera);
    this.scene.overrideMaterial = null;
    this.camera.layers.set(0);
  }

  dispose(): void {
    this.disposed = true;
    this.xrayMaterial.dispose();
    this.missileBody.dispose();
    this.missileNose.dispose();
    this.missileSkin.dispose();
    this.missileTip.dispose();
    cancelAnimationFrame(this.frameHandle);
    window.removeEventListener("resize", this.onResize);
    window.removeEventListener("keydown", this.onKeyDown);
    window.removeEventListener("keyup", this.onKeyUp);
    window.removeEventListener("mousemove", this.onMouseMove);
    window.removeEventListener("mousedown", this.onMouseDown);
    window.removeEventListener("mouseup", this.onMouseUp);
    window.removeEventListener("contextmenu", this.onContextMenu);
    window.removeEventListener("blur", this.onBlur);
    window.removeEventListener("beforeunload", this.onBeforeUnload);
    this.keyboardLock()?.unlock();
    if (document.fullscreenElement) document.exitFullscreen().catch(() => undefined);
    document.removeEventListener("pointerlockchange", this.onLockChange);
    if (document.pointerLockElement) document.exitPointerLock();
    this.hud.dispose();
    this.meter.dispose();
    this.audio.dispose();
    // The art outlives the match; its GPU copies belong to this renderer and go first.
    this.effects.dispose();
    this.viewModel.dispose();
    disposeWorld(this.world);
    releaseGpu();
    releaseCharacterGpu();
    releaseWireGpu();
    setCharacterView(null);
    for (const remote of this.remotes.values()) {
      remote.character.dispose();
      remote.hitbox?.dispose();
    }
    this.renderer.dispose();
    // Whatever the renderer still holds (shader programs, the drawing buffer) goes with the context
    // now, rather than when the page collects it: browsers keep only so many WebGL contexts alive.
    this.renderer.forceContextLoss();
    this.renderer.domElement.remove();
    this.gameRoot.replaceChildren();
  }
}
