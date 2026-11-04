import "./style.css";
import { RoomPhase, type MatchStart, type RoomInfo, type RoomState } from "./gen/game_pb";
import { Connection } from "./net";
import { Lobby } from "./lobby";
import { Game } from "./game/game";
import { artStatus, loadArt } from "./game/art/loader";

const gameRoot = document.getElementById("game-root")!;
const uiRoot = document.getElementById("ui-root")!;
const params = new URLSearchParams(location.search);

if (params.has("art")) {
  // The art preview stands alone and never connects to the server.
  void import("./game/art/preview").then(({ showArtPreview }) => showArtPreview(uiRoot));
} else {
  play();
}

function play(): void {
  const debug = params.has("debug");
  let myId = 0;
  let game: Game | null = null;
  let lastRoom: RoomState | null = null;
  let rooms: RoomInfo[] = [];
  // A match that started before the art was in: still loading, or a failed file having its one
  // retry. It opens once loadArt() is done, which its timeouts bound; until then its snapshots are
  // dropped, which is safe because each one carries the full state.
  let pending: MatchStart | null = null;
  const loadingArt = document.createElement("div");
  loadingArt.className = "art-wait";
  loadingArt.setAttribute("role", "status");
  loadingArt.textContent = "에셋 불러오는 중…";

  // The art loads while the player logs in and picks a room.
  void loadArt();
  if (debug) (window as unknown as { __art: typeof artStatus }).__art = artStatus;

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
          if (!game && !pending) lobby.showRoom(message.value);
          break;
        case "leftRoom":
          lastRoom = null;
          closeGame();
          lobby.showRooms(rooms);
          break;
        case "error":
          lobby.toast(message.value.message);
          break;
        case "matchStart": {
          lobby.hide();
          closeGame();
          const start = message.value;
          pending = start;
          uiRoot.append(loadingArt);
          // With all the art in, this opens the match before the next message is handled, so the
          // notice is never drawn.
          void loadArt().then(() => {
            if (pending === start) startGame(start);
          });
          break;
        }
        case "snapshot":
          game?.onSnapshot(message.value);
          break;
        case "rosterUpdate":
          if (pending) pending.roster = message.value.roster;
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

  function startGame(start: MatchStart): void {
    pending = null;
    loadingArt.remove();
    game = new Game(gameRoot, uiRoot, start, connection);
    if (debug) (window as unknown as { __game: Game }).__game = game;
  }

  /** Ends the match on screen, or one still waiting for the art. */
  function closeGame(): void {
    pending = null;
    loadingArt.remove();
    const closing = game;
    game = null;
    // A cleanup failure must never strand the player outside the room screen.
    try {
      closing?.dispose();
    } catch (error) {
      console.error(error);
    }
  }
}
