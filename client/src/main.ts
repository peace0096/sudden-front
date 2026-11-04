import "./style.css";
import { RoomPhase, type RoomInfo, type RoomState } from "./gen/game_pb";
import { Connection } from "./net";
import { Lobby } from "./lobby";
import { Game } from "./game/game";

const gameRoot = document.getElementById("game-root")!;
const uiRoot = document.getElementById("ui-root")!;

let myId = 0;
let game: Game | null = null;
let lastRoom: RoomState | null = null;
let rooms: RoomInfo[] = [];

const connection: Connection = new Connection(
  () => lobby.showLogin(),
  (message) => {
    if (!message.case) return;
    switch (message.case) {
      case "welcome":
        myId = message.value.playerId;
        lobby.setIdentity(myId, message.value.name);
        lobby.showRooms(rooms);
        break;
      case "roomList":
        rooms = message.value.rooms;
        if (!lastRoom && !game) lobby.showRooms(rooms);
        break;
      case "roomState":
        lastRoom = message.value;
        // The server only sends this after a match once results have finished.
        if (message.value.room?.phase === RoomPhase.WAITING) closeGame();
        if (!game) lobby.showRoom(message.value);
        break;
      case "leftRoom":
        lastRoom = null;
        closeGame();
        lobby.showRooms(rooms);
        break;
      case "error":
        lobby.toast(message.value.message);
        break;
      case "matchStart":
        lobby.hide();
        closeGame();
        game = new Game(gameRoot, uiRoot, message.value, connection);
        if (new URLSearchParams(location.search).has("debug")) {
          (window as unknown as { __game: Game }).__game = game;
        }
        break;
      case "snapshot":
        game?.onSnapshot(message.value);
        break;
      case "rosterUpdate":
        game?.onRoster(message.value.roster);
        break;
      case "matchEnd":
        game?.onMatchEnd(message.value);
        break;
      case "pong":
        game?.onPong(message.value.clientTime);
        break;
    }
  },
  () => {
    closeGame();
    lobby.showDisconnected();
  },
);

const lobby = new Lobby(uiRoot, connection);

function closeGame(): void {
  const closing = game;
  game = null;
  // A cleanup failure must never strand the player outside the room screen.
  try {
    closing?.dispose();
  } catch (error) {
    console.error(error);
  }
}
