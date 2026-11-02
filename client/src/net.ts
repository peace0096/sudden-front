import { create, fromBinary, toBinary, type MessageInitShape } from "@bufbuild/protobuf";
import {
  ClientMessageSchema,
  ServerMessageSchema,
  type ServerMessage,
} from "./gen/game_pb";

type ClientPayload = NonNullable<MessageInitShape<typeof ClientMessageSchema>["payload"]>;

export class Connection {
  private readonly socket: WebSocket;

  constructor(
    onOpen: () => void,
    onMessage: (message: ServerMessage["payload"]) => void,
    onClose: () => void,
  ) {
    const scheme = location.protocol === "https:" ? "wss:" : "ws:";
    this.socket = new WebSocket(`${scheme}//${location.host}/ws`);
    this.socket.binaryType = "arraybuffer";
    this.socket.onopen = onOpen;
    this.socket.onclose = onClose;
    this.socket.onmessage = (event) => {
      onMessage(fromBinary(ServerMessageSchema, new Uint8Array(event.data as ArrayBuffer)).payload);
    };
  }

  send(payload: ClientPayload): void {
    if (this.socket.readyState !== WebSocket.OPEN) return;
    this.socket.send(toBinary(ClientMessageSchema, create(ClientMessageSchema, { payload })));
  }
}
