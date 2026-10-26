/** One event of a text/event-stream: its `id:`, `event:` and joined `data:` lines */
export type SseEvent = { id: string | null; event: string; data: string };

/**
 * Reads a text/event-stream as it arrives, in chunks cut anywhere. `push` takes the next chunk and
 * gives back the events it completed. Comments (`: ping`) and `retry:` lines produce nothing
 */
export class SseParser {
  #buffer = "";
  #id: string | null = null;
  #event = "";
  #data: string[] = [];

  push(chunk: string): SseEvent[] {
    this.#buffer += chunk;
    const events: SseEvent[] = [];
    // a line ends at \n, \r\n or a lone \r; a \r at the very end may still be the start of \r\n
    for (;;) {
      const match = /\r\n|\r|\n/.exec(this.#buffer);
      if (!match) break;
      if (match[0] === "\r" && match.index === this.#buffer.length - 1) break;
      const line = this.#buffer.slice(0, match.index);
      this.#buffer = this.#buffer.slice(match.index + match[0].length);
      const event = this.#line(line);
      if (event) events.push(event);
    }
    return events;
  }

  #line(line: string): SseEvent | null {
    if (line === "") {
      if (this.#data.length === 0 && this.#event === "") return null;
      const event = { id: this.#id, event: this.#event || "message", data: this.#data.join("\n") };
      this.#event = "";
      this.#data = [];
      this.#id = null;
      return event;
    }
    if (line.startsWith(":")) return null;
    const colon = line.indexOf(":");
    const field = colon === -1 ? line : line.slice(0, colon);
    let value = colon === -1 ? "" : line.slice(colon + 1);
    if (value.startsWith(" ")) value = value.slice(1);
    if (field === "event") this.#event = value;
    else if (field === "data") this.#data.push(value);
    else if (field === "id" && !value.includes("\0")) this.#id = value;
    return null;
  }
}
