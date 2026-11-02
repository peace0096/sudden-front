import { Augment, Team, WeaponKind, type MapBox, type ScoreEntry, type WeaponStats } from "../gen/game_pb";
import { AUGMENTS, GRADE_CLASSES, GRADE_NAMES } from "../augments";
import { sidearmNote, weaponCards, weaponTypeName } from "../weapons";

const pad3 = (value: number) => String(value).padStart(3, "0");

const gunIcon = (path: string) => `<svg viewBox="0 0 64 18" aria-hidden="true"><path fill="currentColor" d="${path}"/></svg>`;
const GUN_ICONS: Record<number, string> = {
  [WeaponKind.RIFLE]: gunIcon("M1 6h6l1 2h20l1-2h7V4h3v2h13v2h11v1.6H52V11h-9l-1 1.5h-6l-2 4.5h-5l1.5-5h-5l-3-2H9l-5 3H1z"),
  [WeaponKind.SNIPER]: gunIcon("M1 7h8l2 1h14V6h-2V3h16v3h-2v2h26v1.4H37V11H25l-2 2h-6l2-3H9l-4 3H1z"),
  [WeaponKind.MACHINE_GUN]: gunIcon("M1 5h7l2 2h20V4h9v3h9v1h15v2H48v2h-9v5H26v-5h-3l-2 5h-5l1.5-5H10l-5 3H1z"),
  [WeaponKind.PISTOL]: gunIcon("M24 5h22v3H42v2H33l-1 2h-3l-2 5h-5l2.5-7H24z"),
  [WeaponKind.PISTOL_PLUS]: gunIcon("M24 5h22v3H42v2H33l-1 2h-3l-2 5h-5l2.5-7H24z"),
  [WeaponKind.MAGNUM]: gunIcon("M20 4h34v3H38v5H29l-2 5h-6l3-7h-4z"),
  [WeaponKind.AKIMBO_SMG]: gunIcon("M14 6h28v1h6v2h-6v2H33l-1 6h-5l1-6h-5l-2 4h-4l1.5-4H14L8 12H5V7h3l1 1h5z"),
  [WeaponKind.KNIFE]: gunIcon("M8 7h14v1h4V6h1l27 2-12 4H27v-2h-5v1H8z"),
  [WeaponKind.SHOTGUN]: gunIcon("M1 7h8l2 1h12V6h10v2h29v2H40l-1 2h-9l-1-2h-6l-2 3h-6l2-3H9l-4 3H1z"),
  [WeaponKind.TASER]: gunIcon("M24 5h22v3H42v2H33l-1 2h-3l-2 5h-5l2.5-7H24z"),
  [WeaponKind.MISSILE]: gunIcon("M2 7h6l3-2h38l6-3h8v12h-8l-6-3H36l-1 2h-5l-1 4h-4l1-6H11L8 9H2z"),
  [WeaponKind.SMG]: gunIcon("M14 6h28v1h6v2h-6v2H33l-1 6h-5l1-6h-5l-2 4h-4l1.5-4H14L8 12H5V7h3l1 1h5z"),
};

function div(parent: HTMLElement, className: string, text = ""): HTMLDivElement {
  const node = document.createElement("div");
  node.className = className;
  if (text) node.textContent = text;
  parent.append(node);
  return node;
}

export interface RadarBlip {
  x: number;
  z: number;
  enemy: boolean;
}

export interface KillLine {
  killer: string;
  killerTeam: Team;
  victim: string;
  victimTeam: Team;
  headshot: boolean;
  weapon: WeaponKind;
}

export class Hud {
  readonly root: HTMLDivElement;
  private readonly red: HTMLDivElement;
  private readonly blue: HTMLDivElement;
  private readonly clock: HTMLDivElement;
  private readonly hp: HTMLDivElement;
  private readonly hpBar: HTMLDivElement;
  private readonly mag: HTMLDivElement;
  private readonly reserve: HTMLDivElement;
  private readonly hint: HTMLDivElement;
  private readonly feed: HTMLDivElement;
  private readonly crosshair: HTMLDivElement;
  private readonly hitMarker: HTMLDivElement;
  private readonly hurtVignette: HTMLDivElement;
  private readonly hurtArrow: HTMLDivElement;
  private readonly lowHp: HTMLDivElement;
  private readonly death: HTMLDivElement;
  private readonly deathText: HTMLDivElement;
  private readonly deathTimer: HTMLDivElement;
  private readonly board: HTMLDivElement;
  private readonly pause: HTMLDivElement;
  private readonly radar: HTMLCanvasElement;
  private readonly radarContext: CanvasRenderingContext2D;
  private readonly ping: HTMLDivElement;
  private readonly weaponName: HTMLDivElement;
  private readonly slotBar: HTMLDivElement;
  private readonly weaponMenu: HTMLDivElement;
  private readonly scope: HTMLDivElement;
  private readonly target: HTMLDivElement;
  private readonly stun: HTMLDivElement;
  private killTarget = 30;
  private readonly shieldPanel: HTMLDivElement;
  private readonly shieldValue: HTMLDivElement;
  private readonly badge: HTMLDivElement;
  private readonly offer: HTMLDivElement;
  private readonly offerTimer: HTMLDivElement;
  private readonly offerBody: HTMLDivElement;
  private readonly countdown: HTMLDivElement;
  private hurtTimer = 0;
  private markerTimer = 0;

  constructor(parent: HTMLElement, private readonly boxes: MapBox[], onLeave: () => void, onResume: () => void) {
    this.root = div(parent, "hud");

    const radarWrap = div(this.root, "radar-wrap");
    this.radar = document.createElement("canvas");
    this.radar.width = this.radar.height = 340;
    this.radar.className = "radar";
    radarWrap.append(this.radar);
    this.radarContext = this.radar.getContext("2d")!;
    this.clock = div(radarWrap, "clock", "00:00");
    this.ping = div(radarWrap, "ping");

    const score = div(this.root, "scorebar");
    this.red = div(score, "score red", "000");
    this.target = div(score, "score target", "030");
    this.target.title = "목표 킬 수";
    this.blue = div(score, "score blue", "000");

    this.feed = div(this.root, "killfeed");

    const hpPanel = div(this.root, "hp-panel");
    div(hpPanel, "hp-label", "HP");
    this.hp = div(hpPanel, "hp-value", "100");
    this.hpBar = div(div(hpPanel, "hp-track"), "hp-bar");

    this.shieldPanel = div(hpPanel, "shield-line hidden");
    div(this.shieldPanel, "", "방패");
    this.shieldValue = div(this.shieldPanel, "");
    this.badge = div(this.root, "augment-badge hidden");

    const ammoPanel = div(this.root, "ammo-panel");
    this.slotBar = div(ammoPanel, "slot-bar");
    this.weaponName = div(ammoPanel, "weapon-name");
    const ammo = div(ammoPanel, "ammo-count");
    this.mag = div(ammo, "mag", "30");
    div(ammo, "slash", "/");
    this.reserve = div(ammo, "reserve", "120");

    this.hint = div(this.root, "hint");

    this.crosshair = div(this.root, "crosshair");
    for (const side of ["t", "b", "l", "r"]) div(this.crosshair, `ch ${side}`);
    this.hitMarker = div(this.root, "hitmarker");

    this.scope = div(this.root, "scope hidden");
    this.lowHp = div(this.root, "low-hp");
    this.hurtVignette = div(this.root, "hurt");
    this.hurtArrow = div(this.root, "hurt-arrow");
    this.stun = div(this.root, "stun-overlay hidden");

    this.death = div(this.root, "death hidden");
    this.deathText = div(this.death, "death-text");
    this.deathTimer = div(this.death, "death-timer");

    this.board = div(this.root, "board hidden");
    this.weaponMenu = div(this.root, "weapon-menu hidden");
    this.countdown = div(this.root, "start-countdown hidden");

    this.pause = div(this.root, "pause");
    div(this.pause, "pause-title", "클릭해서 전투 시작");
    div(this.pause, "pause-keys", "WASD 이동 · 마우스 조준 · 좌클릭 공격 · 우클릭 스코프(저격소총) · 1 주무기 · 2 권총 · 3 나이프 · Ctrl/C 앉기 · R 장전 · B 주무기 선택 · Tab 점수판");
    const leave = document.createElement("button");
    leave.className = "btn";
    leave.textContent = "방 나가기";
    leave.onclick = (event) => {
      event.stopPropagation();
      onLeave();
    };
    this.pause.append(leave);
    this.pause.onclick = onResume;

    this.offer = div(this.root, "augment-offer hidden");
    const panel = div(this.offer, "augment-panel");
    const head = div(panel, "augment-head");
    div(head, "augment-title", "증강 선택");
    this.offerTimer = div(head, "augment-timer");
    this.offerBody = div(panel, "augment-body");
  }

  setPaused(paused: boolean): void {
    this.pause.classList.toggle("hidden", !paused);
  }

  setWeaponName(name: string): void {
    this.weaponName.textContent = name;
  }

  /** The 1/2/3 strip above the ammo counter. */
  setSlots(primary: WeaponKind, sidearm: WeaponKind, active: WeaponKind): void {
    this.slotBar.replaceChildren();
    [primary, sidearm, WeaponKind.KNIFE].forEach((kind, index) => {
      const chip = div(this.slotBar, kind === active ? "slot-chip on" : "slot-chip");
      chip.textContent = `${index + 1} ${weaponTypeName(kind)}`;
    });
  }

  showAugmentOffer(cards: Augment[], refreshable: boolean[], onPick: (augment: Augment) => void, onRefresh: (slot: number) => void): void {
    this.offer.classList.remove("hidden");
    this.offerBody.replaceChildren();
    const row = div(this.offerBody, "augment-cards");
    cards.forEach((augment, index) => {
      const info = AUGMENTS[augment];
      if (!info) return;
      const slot = div(row, "augment-slot");
      const card = document.createElement("button");
      card.type = "button";
      card.className = `augment-card ${GRADE_CLASSES[info.grade]}`;
      card.onclick = (event) => {
        event.stopPropagation();
        onPick(augment);
      };
      div(card, "augment-grade", GRADE_NAMES[info.grade]);
      div(card, "augment-name", info.name);
      div(card, "augment-text", info.description);
      div(card, "augment-key", String(index + 1));
      slot.append(card);
      const refresh = document.createElement("button");
      refresh.type = "button";
      refresh.className = "btn augment-refresh";
      refresh.textContent = refreshable[index] ? "변경" : "변경 완료";
      refresh.disabled = !refreshable[index];
      refresh.onclick = (event) => {
        event.stopPropagation();
        onRefresh(index);
      };
      slot.append(refresh);
    });
  }

  hideAugmentOffer(): void {
    this.offer.classList.add("hidden");
  }

  setOfferTimer(seconds: number): void {
    this.offerTimer.textContent = `${seconds}초 안에 선택 · 고르지 않으면 무작위`;
  }

  setStun(seconds: number): void {
    this.stun.classList.toggle("hidden", seconds <= 0);
    if (seconds > 0) this.stun.textContent = `감전! 마비 ${Math.ceil(seconds)}초`;
  }

  setCountdown(text: string): void {
    this.countdown.classList.toggle("hidden", !text);
    this.countdown.textContent = text;
  }

  setAugmentBadge(augment: Augment, active: Augment[]): void {
    const info = AUGMENTS[augment];
    this.badge.classList.toggle("hidden", !info);
    if (!info) return;
    this.badge.className = `augment-badge ${GRADE_CLASSES[info.grade]}`;
    const granted = active.filter((a) => a !== augment).map((a) => AUGMENTS[a]?.name ?? "");
    this.badge.textContent = granted.length > 0 ? `${info.name}: ${granted.join(" + ")}` : info.name;
  }

  setShield(hp: number): void {
    this.shieldPanel.classList.toggle("hidden", hp <= 0);
    this.shieldValue.textContent = String(hp);
  }

  setScope(on: boolean): void {
    this.scope.classList.toggle("hidden", !on);
  }

  showWeaponMenu(weapons: WeaponStats[], selected: WeaponKind, onPick: (kind: WeaponKind) => void, note: string): void {
    this.weaponMenu.replaceChildren();
    this.weaponMenu.classList.remove("hidden");
    const panel = div(this.weaponMenu, "weapon-menu-panel");
    div(panel, "weapon-menu-title", "주무기 선택");
    div(panel, "weapon-menu-note", note);
    panel.append(weaponCards(weapons, selected, onPick, true), sidearmNote());
    div(panel, "weapon-menu-keys", "1~4 또는 클릭으로 선택 · B / Esc 닫기");
  }

  hideWeaponMenu(): void {
    this.weaponMenu.classList.add("hidden");
  }

  setKillTarget(kills: number): void {
    this.killTarget = kills;
    this.target.textContent = String(kills).padStart(3, "0");
  }

  setScore(red: number, blue: number): void {
    this.red.textContent = pad3(red);
    this.blue.textContent = pad3(blue);
  }

  setClock(seconds: number): void {
    const m = Math.floor(seconds / 60);
    const s = Math.floor(seconds % 60);
    this.clock.textContent = `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }

  setPing(ms: number): void {
    this.ping.textContent = `${Math.round(ms)} ms`;
  }

  setHealth(hp: number, maxHp: number): void {
    this.hp.textContent = String(hp);
    this.hpBar.style.width = `${(100 * hp) / maxHp}%`;
    const low = hp > 0 && hp <= 30;
    this.hp.classList.toggle("low", low);
    this.hpBar.classList.toggle("low", low);
    this.lowHp.classList.toggle("on", low);
  }

  setAmmo(magazine: number, reserve: number, reloadRemaining: number, reloadTime: number, melee: boolean, infinite: boolean, lowAt: number): void {
    if (melee) {
      this.mag.textContent = "—";
      this.reserve.textContent = "—";
      this.mag.classList.remove("low");
      this.hint.textContent = "";
      this.hint.className = "hint";
      return;
    }
    this.mag.textContent = String(magazine);
    this.reserve.textContent = infinite ? "∞" : String(reserve);
    this.mag.classList.toggle("low", magazine <= lowAt);
    if (reloadRemaining > 0) {
      this.hint.textContent = `장전 중… ${reloadRemaining.toFixed(1)}초`;
      this.hint.style.setProperty("--progress", `${100 * (1 - reloadRemaining / reloadTime)}%`);
      this.hint.className = "hint reloading";
    } else if (magazine === 0 && reserve === 0 && !infinite) {
      this.hint.textContent = "탄약 없음";
      this.hint.className = "hint warn";
    } else {
      this.hint.textContent = "";
      this.hint.className = "hint";
    }
  }

  setCrosshair(spreadPx: number, visible: boolean): void {
    this.crosshair.style.setProperty("--gap", `${spreadPx}px`);
    this.crosshair.classList.toggle("hidden", !visible);
  }

  showHit(headshot: boolean, kill: boolean): void {
    this.hitMarker.className = `hitmarker on${headshot ? " head" : ""}${kill ? " kill" : ""}`;
    this.markerTimer = 0.18;
  }

  showHurt(relativeAngle: number): void {
    this.hurtTimer = 0.6;
    this.hurtArrow.style.transform = `translate(-50%, -50%) rotate(${relativeAngle}rad)`;
  }

  update(dt: number): void {
    if (this.markerTimer > 0) {
      this.markerTimer -= dt;
      if (this.markerTimer <= 0) this.hitMarker.className = "hitmarker";
    }
    this.hurtTimer = Math.max(0, this.hurtTimer - dt);
    const strength = Math.min(1, this.hurtTimer / 0.4);
    this.hurtVignette.style.opacity = String(strength * 0.8);
    this.hurtArrow.style.opacity = String(strength);
  }

  addKill(line: KillLine): void {
    const row = div(this.feed, "kill-row");
    if (line.headshot) div(row, "headshot", "[HEAD SHOT]");
    div(row, line.killerTeam === Team.RED ? "name red" : "name blue", line.killer);
    const icon = div(row, "gun-icon");
    icon.innerHTML = GUN_ICONS[line.weapon] ?? GUN_ICONS[WeaponKind.RIFLE];
    div(row, line.victimTeam === Team.RED ? "name red" : "name blue", line.victim);
    window.setTimeout(() => row.remove(), 6000);
    while (this.feed.children.length > 6) this.feed.firstElementChild?.remove();
  }

  showDeath(killer: string | null, respawnIn: number): void {
    this.death.classList.remove("hidden");
    this.deathText.textContent = killer ? `${killer}에게 사살되었습니다` : "사살되었습니다";
    this.deathTimer.textContent = `${Math.ceil(respawnIn)}초 후 재투입 · B: 주무기 변경`;
  }

  hideDeath(): void {
    this.death.classList.add("hidden");
  }

  showBoard(show: boolean): void {
    this.board.classList.toggle("hidden", !show);
  }

  renderBoard(
    entries: ScoreEntry[],
    red: number,
    blue: number,
    myId: number,
    banner?: { title: string; subtitle: string; victory: boolean },
  ): void {
    this.board.replaceChildren();
    const panel = div(this.board, "board-panel");
    div(panel, "board-title", banner?.title ?? "TEAM DEATHMATCH");
    const subtitle = div(panel, "board-sub", banner?.subtitle ?? `${this.killTarget}킬 선취 승리`);
    if (banner) subtitle.classList.add(banner.victory ? "win" : "lose");
    const columns = div(panel, "board-teams");
    for (const [team, label, cls, score] of [
      [Team.RED, "RED TEAM", "red", red],
      [Team.BLUE, "BLUE TEAM", "blue", blue],
    ] as const) {
      const column = div(columns, `board-team ${cls}`);
      div(column, "board-team-name", label);
      const head = div(column, "board-row head");
      div(head, "n", "닉네임");
      div(head, "k", "킬");
      div(head, "d", "데스");
      const rows = entries
        .filter((entry) => entry.team === team)
        .sort((a, b) => b.kills - a.kills || a.deaths - b.deaths);
      for (const entry of rows) {
        const row = div(column, entry.id === myId ? "board-row me" : "board-row");
        div(row, "n", entry.name);
        div(row, "k", String(entry.kills));
        div(row, "d", String(entry.deaths));
      }
      div(column, "board-total", `킬수 : ${score}`);
    }
  }

  drawRadar(x: number, z: number, yaw: number, blips: RadarBlip[]): void {
    const g = this.radarContext;
    const size = this.radar.width;
    const c = size / 2;
    const radius = c - 6;
    const range = 34;
    const scale = radius / range;

    g.clearRect(0, 0, size, size);
    g.save();
    g.beginPath();
    g.arc(c, c, radius, 0, Math.PI * 2);
    g.clip();
    g.fillStyle = "rgba(14,16,14,0.78)";
    g.fillRect(0, 0, size, size);

    // View cone stays pointing up; the world turns under it.
    const cone = g.createRadialGradient(c, c, 0, c, c, radius);
    cone.addColorStop(0, "rgba(216,66,48,0.62)");
    cone.addColorStop(1, "rgba(216,66,48,0.08)");
    g.fillStyle = cone;
    g.beginPath();
    g.moveTo(c, c);
    g.arc(c, c, radius, -Math.PI / 2 - 0.62, -Math.PI / 2 + 0.62);
    g.closePath();
    g.fill();

    const toRadar = (wx: number, wz: number): [number, number] => {
      const dx = wx - x;
      const dz = wz - z;
      const cos = Math.cos(yaw);
      const sin = Math.sin(yaw);
      return [c + (dx * cos - dz * sin) * scale, c + (dx * sin + dz * cos) * scale];
    };

    g.fillStyle = "rgba(205,205,185,0.3)";
    for (const b of this.boxes) {
      if (b.minY >= 1.8) continue;
      const p1 = toRadar(b.minX, b.minZ);
      const p2 = toRadar(b.maxX, b.minZ);
      const p3 = toRadar(b.maxX, b.maxZ);
      const p4 = toRadar(b.minX, b.maxZ);
      g.beginPath();
      g.moveTo(p1[0], p1[1]);
      g.lineTo(p2[0], p2[1]);
      g.lineTo(p3[0], p3[1]);
      g.lineTo(p4[0], p4[1]);
      g.closePath();
      g.fill();
    }

    for (const blip of blips) {
      const [bx, by] = toRadar(blip.x, blip.z);
      g.fillStyle = blip.enemy ? "#ff4a3a" : "#7fd4ff";
      g.beginPath();
      g.arc(bx, by, 7, 0, Math.PI * 2);
      g.fill();
    }
    g.restore();

    g.strokeStyle = "rgba(200,200,180,0.65)";
    g.lineWidth = 4;
    g.beginPath();
    g.arc(c, c, radius, 0, Math.PI * 2);
    g.stroke();

    g.fillStyle = "#f4f0d8";
    g.beginPath();
    g.moveTo(c, c - 11);
    g.lineTo(c + 8, c + 8);
    g.lineTo(c, c + 3);
    g.lineTo(c - 8, c + 8);
    g.closePath();
    g.fill();

    // Compass letter rides the rim. North is -Z.
    const north = toRadar(x, z - 100);
    const angle = Math.atan2(north[1] - c, north[0] - c);
    g.font = "bold 28px Arial";
    g.textAlign = "center";
    g.textBaseline = "middle";
    g.fillStyle = "#ffffff";
    g.fillText("N", c + Math.cos(angle) * (radius - 20), c + Math.sin(angle) * (radius - 20));
  }

  dispose(): void {
    this.root.remove();
  }
}
