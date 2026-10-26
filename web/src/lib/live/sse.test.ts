import { describe, expect, it } from "vitest";

import { SseParser } from "./sse";

describe("SseParser", () => {
  it("reads events cut at any byte", () => {
    const text =
      'retry: 1000\n\nid: 7\nevent: hello\ndata: {"a":1}\n\n: ping\n\nid: 8\nevent: lagged\ndata: {}\n\n';
    for (let size = 1; size <= text.length; size++) {
      const parser = new SseParser();
      const events = [];
      for (let i = 0; i < text.length; i += size)
        events.push(...parser.push(text.slice(i, i + size)));
      expect(events).toEqual([
        { id: "7", event: "hello", data: '{"a":1}' },
        { id: "8", event: "lagged", data: "{}" }
      ]);
    }
  });

  it("joins data lines and takes every line ending", () => {
    const parser = new SseParser();
    expect(parser.push("data: one\r\ndata:two\rdata:  three\r")).toEqual([]);
    expect(parser.push("\n\r\n")).toEqual([
      { id: null, event: "message", data: "one\ntwo\n three" }
    ]);
  });

  it("ignores a blank line with nothing before it", () => {
    expect(new SseParser().push("\n\n\n")).toEqual([]);
  });
});
