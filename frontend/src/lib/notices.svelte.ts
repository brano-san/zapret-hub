import type { Notice } from "./ipc/bindings";

/** One queue for every error and message (spec 10): errors first, nothing dropped, closing shows the next. */
export class NoticeQueue {
  items = $state<Notice[]>([]);

  get current(): Notice | undefined {
    return this.items[0];
  }

  push(notice: Notice): void {
    if (notice.level !== "Error") {
      this.items.push(notice);
      return;
    }
    const firstMessage = this.items.findIndex((item) => item.level !== "Error");
    this.items.splice(firstMessage === -1 ? this.items.length : firstMessage, 0, notice);
  }

  dismiss(): void {
    this.items.shift();
  }
}

export const notices = new NoticeQueue();
