import {
  GameMode,
  MapId,
  RoomPhase,
  Team,
  type RoomInfo,
  type RoomState,
} from "./gen/game_pb";
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

const TEAM_NAMES: Record<number, string> = {
  [Team.RED]: "RED",
  [Team.BLUE]: "BLUE",
};

const KILL_TARGETS = [10, 20, 30, 50];

function botNote(room: RoomInfo): string {
  return room.botCount > 0 ? ` · 봇 ${room.botCount}` : "";
}

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

  constructor(
    private readonly root: HTMLElement,
    private readonly connection: Connection,
  ) {
    root.append(this.screen);
  }

  setIdentity(id: number, name: string): void {
    this.myId = id;
    this.myName = name;
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
    // The list refreshes whenever any room changes. With the lobby already up only the list is
    // redrawn, so the create form keeps its text, an open dropdown and the focus.
    const shown = this.screen.querySelector(".room-list");
    if (shown && !this.screen.classList.contains("hidden")) {
      shown.replaceWith(this.roomListPanel(rooms));
      return;
    }

    const frame = this.reset();
    const columns = el("div", "columns");

    const createPanel = el("form", "panel create-room");
    createPanel.append(el("h2", "", "방 만들기"));
    const name = el("input", "field room-name-input");
    name.placeholder = "방 제목";
    name.maxLength = 24;
    name.setAttribute("aria-label", "방 제목");
    const label = el("label", "check");
    const bots = el("input", "bots-check");
    bots.type = "checkbox";
    bots.checked = true;
    label.append(bots, el("span", "", "빈 자리는 봇으로 채우기"));
    const mapSelect = el("select", "field map-select");
    mapSelect.setAttribute("aria-label", "맵 선택");
    for (const id of [MapId.SUPPLY_YARD, MapId.WAREHOUSE]) {
      const option = el("option", "", MAP_NAMES[id]);
      option.value = String(id);
      mapSelect.append(option);
    }
    const modeSelect = el("select", "field mode-select");
    modeSelect.setAttribute("aria-label", "게임 모드 선택");
    for (const id of [GameMode.NORMAL, GameMode.AUGMENT]) {
      const option = el("option", "", MODE_NAMES[id]);
      option.value = String(id);
      modeSelect.append(option);
    }
    createPanel.append(name, modeSelect, mapSelect, label, el("button", "btn primary", "방 생성"));
    createPanel.onsubmit = (event) => {
      event.preventDefault();
      this.connection.send({
        case: "createRoom",
        value: { name: name.value, fillBots: bots.checked, map: Number(mapSelect.value) as MapId, mode: Number(modeSelect.value) as GameMode },
      });
    };

    columns.append(this.roomListPanel(rooms), createPanel);
    frame.append(columns);
  }

  private roomListPanel(rooms: RoomInfo[]): HTMLElement {
    const panel = el("section", "panel room-list");
    const heading = el("div", "panel-head");
    heading.append(el("h2", "", "방 목록"), el("span", "muted", `${rooms.length}개 · ${this.myName}`));
    panel.append(heading);

    if (rooms.length === 0) {
      panel.append(el("p", "empty", "열려 있는 방이 없습니다. 첫 방을 만들어 보세요."));
    }
    for (const room of rooms) {
      const inGame = room.phase === RoomPhase.IN_GAME;
      const full = room.playerCount >= room.maxPlayers;
      const row = el("div", "room-row");
      const info = el("div", "room-info");
      info.append(
        el("strong", "", room.name),
        el("span", "muted", `${MODE_NAMES[room.mode] ?? ""} · ${MAP_NAMES[room.map] ?? ""} · ${room.killsToWin}킬 · 방장 ${room.hostName}${botNote(room)}`),
      );
      const count = el("span", "room-count", `${room.playerCount}/${room.maxPlayers}`);
      const status = el("span", inGame ? "status live" : "status", inGame ? "게임중" : "대기중");
      const join = el("button", "btn", "입장");
      join.disabled = inGame || full;
      join.onclick = () => this.connection.send({ case: "joinRoom", value: { roomId: room.id } });
      row.append(info, count, status, join);
      panel.append(row);
    }
    return panel;
  }

  showRoom(state: RoomState): void {
    const room = state.room;
    if (!room) return;
    // A click on a seat redraws the whole room; the keyboard should stay on that seat.
    const focused = document.activeElement instanceof HTMLElement ? document.activeElement.dataset.seat : undefined;
    const frame = this.reset();
    const panel = el("section", "panel room");
    const head = el("div", "panel-head");
    head.append(
      el("h2", "", room.name),
      el("span", "muted", `${MODE_NAMES[room.mode] ?? ""} · ${MAP_NAMES[room.map] ?? ""} · ${room.playerCount}/${room.maxPlayers} · ${room.killsToWin}킬 선취 승리${botNote(room)}`),
    );
    panel.append(head);

    const isHost = state.players.some((p) => p.id === this.myId && p.isHost);
    const teams = el("div", "teams");
    teams.append(
      this.teamColumn("RED TEAM", "red", Team.RED, state, isHost),
      this.teamColumn("BLUE TEAM", "blue", Team.BLUE, state, isHost),
    );
    panel.append(teams);

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
    if (focused) frame.querySelector<HTMLElement>(`[data-seat="${focused}"]`)?.focus();
  }

  private teamColumn(title: string, className: string, team: Team, state: RoomState, isHost: boolean): HTMLElement {
    const column = el("div", `team ${className}`);
    column.append(el("h3", "", title));
    for (let slot = 0; slot < TEAM_SIZE; slot++) {
      const player = state.players.find((p) => p.team === team && p.slot === slot);
      if (!player) {
        const bot = state.bots.some((b) => b.team === team && b.slot === slot);
        column.append(this.emptySeat(team, slot, bot, isHost));
        continue;
      }
      const row = el("div", "slot");
      row.append(el("span", "", player.name));
      if (player.isHost) row.append(el("span", "host-tag", "방장"));
      if (player.id === this.myId) row.classList.add("me");
      column.append(row);
    }
    return column;
  }

  // A seat nobody sits in holds a bot or stays open. The host flips it with a click.
  private emptySeat(team: Team, slot: number, bot: boolean, isHost: boolean): HTMLElement {
    const text = bot ? "BOT" : "빈 자리";
    const kind = bot ? "slot bot" : "slot open";
    if (!isHost) {
      const row = el("div", kind);
      row.append(el("span", "muted", text));
      return row;
    }
    const action = bot ? "봇 ✕" : "+ 봇";
    const row = el("button", `${kind} seat-btn`);
    row.type = "button";
    row.dataset.seat = `${team}:${slot}`;
    // The name holds both visible texts, after saying which seat this is.
    row.setAttribute("aria-label", `${TEAM_NAMES[team]} ${slot + 1}번 자리 ${text} ${action}`);
    row.onclick = () => this.connection.send({ case: "setBotSlot", value: { team, slot, bot: !bot } });
    row.append(el("span", "muted", text), el("span", "seat-action", action));
    return row;
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
