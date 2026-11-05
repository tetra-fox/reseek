import { describe, expect, it } from "vitest";

import type { Schemas } from "#lib/api/client.js";

import { toDraft } from "./fields";
import type { Field } from "./sections";
import {
  cellOf,
  type Column,
  columnsOf,
  draftOf,
  rowErrors,
  rowProblems,
  rowsOf,
  tableValue,
  typedNumber,
  withCell
} from "./tables";

const column = (key: string, kind: Column["kind"] = "text"): Column => ({
  key,
  label: key,
  doc: "",
  kind,
  choices: [],
  default: null
});

describe("tables", () => {
  it("take their columns from the daemon's description, in the form's words", () => {
    const field: Field = { key: "x", label: "X", columns: { "run.command": "Shell command" } };
    const entry = (key: string, more: object = {}) =>
      ({
        key,
        kind: "string",
        doc: "",
        choices: null,
        default: null,
        secret: false,
        ...more
      }) as unknown as Schemas["EntryView"];
    const columns = columnsOf(field, [
      entry("run.command"),
      entry("token", { secret: true }),
      entry("role", { choices: ["a"], default: "a" }),
      entry("on", { kind: { list: "string" } }),
      entry("n", { kind: "integer", default: 1 }),
      entry("b", { kind: "bool", default: true })
    ]);
    expect(columns[0].label).toBe("Shell command");
    expect(columns.map((c) => c.kind)).toEqual([
      "text",
      "secret",
      "select",
      "list",
      "integer",
      "bool"
    ]);
  });

  it("read the same however a value's keys were ordered, so an undone edit is no change", () => {
    const running = toDraft("map", { a: { role: "x", cidr: "y", key: null } });
    expect(toDraft("map", { a: { cidr: "y", role: "x" } })).toBe(running);
    const [row] = rowsOf("map", String(running));
    const edited = withCell(row, "role", "z");
    expect(draftOf("map", [edited])).not.toBe(running);
    expect(draftOf("map", [withCell(edited, "role", "x")])).toBe(running);
  });

  it("set and clear a nested field, taking out a table left empty", () => {
    const [row] = rowsOf("table-list", '[{"run":{"command":"ls"},"timeout":"1m"}]');
    expect(cellOf(row, "run.command")).toBe("ls");
    const cleared = withCell(row, "run.command", "");
    expect(cleared.value).toEqual({ timeout: "1m" });
    expect(withCell(cleared, "run.args", "-l").value).toEqual({
      timeout: "1m",
      run: { args: "-l" }
    });
    expect(withCell(row, "on", ["a"]).value.on).toEqual(["a"]);
    expect(withCell(withCell(row, "on", ["a"]), "on", []).value.on).toBeUndefined();
  });

  it("find what a person can fix, row by row", () => {
    const columns = [column("slots", "number")];
    const rows = rowsOf(
      "map",
      '[["",{}],["a",{}],["a",{}],["b",{"slots":"many"}],["c",{"slots":2}]]'
    );
    expect([...rowProblems("map", rows, columns).entries()]).toEqual([
      [0, "Give this row a name"],
      [1, "Another row has this name"],
      [2, "Another row has this name"],
      [3, "slots must be a number"]
    ]);
    expect(tableValue("map", '[["",{}]]', columns)).toEqual({ error: "Give this row a name" });
    const whole = [column("slots", "integer")];
    expect(tableValue("map", '[["a",{"slots":1.5}]]', whole)).toEqual({
      error: "slots must be a whole number"
    });
    expect(tableValue("map", '[["a",{"slots":"x"}]]', whole)).toEqual({
      error: "slots must be a number"
    });
    expect(tableValue("map", '[[" a ",{"slots":2}]]', columns)).toEqual({
      set: { a: { slots: 2 } }
    });
    expect([typedNumber("12"), typedNumber("12x"), typedNumber("")]).toEqual([12, "12x", ""]);
  });

  it("place the daemon's messages at the rows they name, the rest apart", () => {
    const key = "web.authentication.api_keys";
    const rows = rowsOf("map", '[["robot",{}],["robot2",{}]]');
    const { byRow, rest } = rowErrors(
      key,
      "map",
      rows,
      `The field ${key}.robot2.key must contain a value.; ` +
        `The field ${key}.robot.key must contain a value.; Something else.`
    );
    expect(byRow.get(rows[0].id)).toContain(`${key}.robot.key`);
    expect(byRow.get(rows[1].id)).toContain(`${key}.robot2.key`);
    expect(rest).toEqual(["Something else."]);
    // a row whose name starts another's takes only what names it
    const dotted = rowsOf("map", '[["a",{}],["a.b",{}]]');
    const nested = rowErrors(key, "map", dotted, `The field ${key}.a.b.key must contain a value.`);
    expect([...nested.byRow.keys()]).toEqual([dotted[1].id]);
    const list = rowsOf("table-list", "[{},{}]");
    expect(
      rowErrors("x", "table-list", list, "The field x[1].token is hidden.").byRow.get(list[1].id)
    ).toContain("x[1]");
  });
});
