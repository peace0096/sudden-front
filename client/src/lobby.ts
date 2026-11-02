import {
  GameMode,
  MapId,
  RoomPhase,
  Team,
  WeaponKind,
  type RoomInfo,
  type RoomPlayer,
  type RoomState,
  type WeaponStats,
} from "./gen/game_pb";
import { sidearmNote, weaponCards, weaponTypeName } from "./weapons";
import type { Connection } from "./net";

const NAME_KEY = "sudden-front-name";
const TEAM_SIZE = 5;
const MAP_NAMES: Record<number, string> = {
  [MapId.SUPPLY_YARD]: "보급창고",
  [MapId.WAREHOUSE]: "웨어하우스",
};

const MODE_NAMES: Record<number, string> = {
  [GameMode.NORMAL]: "일반모드",
  [GameMode.AUGMENT]: "증강모드",
};

const KILL_TARGETS = [10, 20, 30, 50];

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className = "",
  text = "",
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

function loadName(): string {
  try {
    return localStorage.getItem(NAME_KEY) ?? "";
  } catch {
    return "";
  }
}

function saveName(name: string): void {
  try {
    localStorage.setItem(NAME_KEY, name);
  } catch {
    // Remembering the name is only a convenience.
  }
}

export class Lobby {
  private readonly screen = el("div", "lobby");
  private myId = 0;
  private myName = "";
  private weapons: WeaponStats[] = [];

  constructor(
    private readonly root: HTMLElement,
    private readonly connection: Connection,
  ) {
    root.append(this.screen);
  }

  setIdentity(id: number, name: string, weapons: WeaponStats[]): void {
    this.myId = id;
    this.myName = name;
    this.weapons = weapons;
  }

  hide(): void {
    this.screen.classList.add("hidden");
  }

  private reset(): HTMLElement {
    this.screen.classList.remove("hidden");
    this.screen.replaceChildren();
    const frame = el("div", "lobby-frame");
    const brand = el("header", "brand");
    brand.append(el("h1", "", "SUDDEN FRONT"), el("p", "", "5 VS 5 TEAM DEATHMATCH"));
    frame.append(brand);
    this.screen.append(frame);
    return frame;
  }

  showLogin(): void {
    const frame = this.reset();
    const panel = el("form", "panel login");
    panel.append(el("h2", "", "입장"));
    const input = el("input", "field");
    input.placeholder = "닉네임 (최대 16자)";
    input.maxLength = 16;
    input.value = loadName();
    input.setAttribute("aria-label", "닉네임");
    const button = el("button", "btn primary", "접속");
    panel.append(input, button);
    panel.onsubmit = (event) => {
      event.preventDefault();
      saveName(input.value.trim());
      this.connection.send({ case: "hello", value: { name: input.value } });
    };
    frame.append(panel);
    input.focus();
  }

  showRooms(rooms: RoomInfo[]): void {
    // Keep what the player is typing when the list refreshes behind the form.
    const previous = this.screen.querySelector<HTMLInputElement>(".room-name-input");
    const typed = previous?.value ?? "";
    const previousBots = this.screen.querySelector<HTMLInputElement>(".bots-check");
    const wantBots = previousBots?.checked ?? true;
    const previousMap = this.screen.querySelector<HTMLSelectElement>(".map-select")?.value;
    const previousMode = this.screen.querySelector<HTMLSelectElement>(".mode-select")?.value;
    const hadFocus = previous !== null && document.activeElement === previous;

    const frame = this.reset();
    const columns = el("div", "columns");

    const listPanel = el("section", "panel room-list");
    const heading = el("div", "panel-head");
    heading.append(el("h2", "", "방 목록"), el("span", "muted", `${rooms.length}개 · ${this.myName}`));
    listPanel.append(heading);

    if (rooms.length === 0) {
      listPanel.append(el("p", "empty", "열려 있는 방이 없습니다. 첫 방을 만들어 보세요."));
    }
    for (const room of rooms) {
      const inGame = room.phase === RoomPhase.IN_GAME;
      const full = room.playerCount >= room.maxPlayers;
      const row = el("div", "room-row");
      const info = el("div", "room-info");
      info.append(
        el("strong", "", room.name),
        el("span", "muted", `${MODE_NAMES[room.mode] ?? ""} · ${MAP_NAMES[room.map] ?? ""} · ${room.killsToWin}킬 · 방장 ${room.hostName}${room.fillBots ? " · 봇 충원" : ""}`),
      );
      const count = el("span", "room-count", `${room.playerCount}/${room.maxPlayers}`);
      const status = el("span", inGame ? "status live" : "status", inGame ? "게임중" : "대기중");
      const join = el("button", "btn", "입장");
      join.disabled = inGame || full;
      join.onclick = () => this.connection.send({ case: "joinRoom", value: { roomId: room.id } });
      row.append(info, count, status, join);
      listPanel.append(row);
    }

    const createPanel = el("form", "panel create-room");
    createPanel.append(el("h2", "", "방 만들기"));
    const name = el("input", "field room-name-input");
    name.placeholder = "방 제목";
    name.maxLength = 24;
    name.value = typed;
    name.setAttribute("aria-label", "방 제목");
    const label = el("label", "check");
    const bots = el("input", "bots-check");
    bots.type = "checkbox";
    bots.checked = wantBots;
    label.append(bots, el("span", "", "빈 자리는 봇으로 채우기"));
    const mapSelect = el("select", "field map-select");
    mapSelect.setAttribute("aria-label", "맵 선택");
    for (const id of [MapId.SUPPLY_YARD, MapId.WAREHOUSE]) {
      const option = el("option", "", MAP_NAMES[id]);
      option.value = String(id);
      mapSelect.append(option);
    }
    if (previousMap) mapSelect.value = previousMap;
    const modeSelect = el("select", "field mode-select");
    modeSelect.setAttribute("aria-label", "게임 모드 선택");
    for (const id of [GameMode.NORMAL, GameMode.AUGMENT]) {
      const option = el("option", "", MODE_NAMES[id]);
      option.value = String(id);
      modeSelect.append(option);
    }
    if (previousMode) modeSelect.value = previousMode;
    createPanel.append(name, modeSelect, mapSelect, label, el("button", "btn primary", "방 생성"));
    createPanel.onsubmit = (event) => {
      event.preventDefault();
      this.connection.send({
        case: "createRoom",
        value: { name: name.value, fillBots: bots.checked, map: Number(mapSelect.value) as MapId, mode: Number(modeSelect.value) as GameMode },
      });
    };

    columns.append(listPanel, createPanel);
    frame.append(columns);
    if (hadFocus) name.focus();
  }

  showRoom(state: RoomState): void {
    const room = state.room;
    if (!room) return;
    const frame = this.reset();
    const panel = el("section", "panel room");
    const head = el("div", "panel-head");
    head.append(
      el("h2", "", room.name),
      el("span", "muted", `${MODE_NAMES[room.mode] ?? ""} · ${MAP_NAMES[room.map] ?? ""} · ${room.playerCount}/${room.maxPlayers} · ${room.killsToWin}킬 선취 승리${room.fillBots ? " · 봇 충원" : ""}`),
    );
    panel.append(head);

    const teams = el("div", "teams");
    teams.append(
      this.teamColumn("RED TEAM", "red", state.players.filter((p) => p.team === Team.RED)),
      this.teamColumn("BLUE TEAM", "blue", state.players.filter((p) => p.team === Team.BLUE)),
    );
    panel.append(teams);

    const me = state.players.find((p) => p.id === this.myId);
    panel.append(el("h3", "pick-title", "주무기 선택 (게임 중에는 B키)"));
    panel.append(
      weaponCards(this.weapons, me?.weapon ?? WeaponKind.RIFLE, (kind) =>
        this.connection.send({ case: "selectWeapon", value: { weapon: kind } }),
      ),
      sidearmNote(),
    );

    const isHost = state.players.some((p) => p.id === this.myId && p.isHost);
    const actions = el("div", "actions");
    if (isHost) {
      const target = el("label", "kill-target");
      const select = el("select", "field kill-target-select");
      for (const kills of KILL_TARGETS) {
        const option = el("option", "", `${kills}킬`);
        option.value = String(kills);
        select.append(option);
      }
      select.value = String(room.killsToWin);
      select.onchange = () => this.connection.send({ case: "setKillsToWin", value: { kills: Number(select.value) } });
      target.append(el("span", "muted", "목표 킬 수"), select);
      actions.append(target);
    }
    const leave = el("button", "btn", "나가기");
    leave.onclick = () => this.connection.send({ case: "leaveRoom", value: {} });
    const swap = el("button", "btn", "팀 변경");
    swap.onclick = () => this.connection.send({ case: "switchTeam", value: {} });
    actions.append(leave, swap);
    if (isHost) {
      const start = el("button", "btn primary", "게임 시작");
      start.onclick = () => this.connection.send({ case: "startGame", value: {} });
      actions.append(start);
    } else {
      actions.append(el("span", "muted waiting", "방장이 시작하기를 기다리는 중…"));
    }
    panel.append(actions);
    frame.append(panel);
  }

  private teamColumn(title: string, className: string, players: RoomPlayer[]): HTMLElement {
    const column = el("div", `team ${className}`);
    column.append(el("h3", "", title));
    for (let slot = 0; slot < TEAM_SIZE; slot++) {
      const player = players[slot];
      const row = el("div", player ? "slot" : "slot open");
      if (player) {
        row.append(el("span", "", player.name));
        const tags = el("span", "slot-tags");
        tags.append(el("span", "muted", weaponTypeName(player.weapon)));
        if (player.isHost) tags.append(el("span", "host-tag", "방장"));
        row.append(tags);
        if (player.id === this.myId) row.classList.add("me");
      } else {
        row.append(el("span", "muted", "빈 자리"));
      }
      column.append(row);
    }
    return column;
  }

  showDisconnected(): void {
    const frame = this.reset();
    const panel = el("section", "panel login");
    panel.append(el("h2", "", "서버 연결이 끊어졌습니다"));
    const retry = el("button", "btn primary", "다시 접속");
    retry.onclick = () => location.reload();
    panel.append(retry);
    frame.append(panel);
  }

  toast(message: string): void {
    const toast = el("div", "toast", message);
    toast.setAttribute("role", "alert");
    this.root.append(toast);
    window.setTimeout(() => toast.remove(), 3000);
  }
}
