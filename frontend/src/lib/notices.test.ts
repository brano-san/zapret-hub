import { describe, expect, it } from "vitest";
import { NoticeQueue } from "./notices.svelte";

describe("NoticeQueue", () => {
  it("shows errors before messages and drops nothing", () => {
    const queue = new NoticeQueue();
    queue.push({ level: "Info", title: "a", text: "" });
    queue.push({ level: "Error", title: "b", text: "" });
    queue.push({ level: "Info", title: "c", text: "" });
    queue.push({ level: "Error", title: "d", text: "" });
    const seen: string[] = [];
    while (queue.current) {
      seen.push(queue.current.title);
      queue.dismiss();
    }
    expect(seen).toEqual(["b", "d", "a", "c"]);
  });
});
