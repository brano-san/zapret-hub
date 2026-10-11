import { localizeError } from "./format";
import { notices } from "./notices.svelte";

/** What tauri-specta makes of a command that returns `Result<T, String>`. */
export type Answer<T> = { status: "ok"; data: T } | { status: "error"; error: string };

/** The command's data, or `undefined` once its error is in the notice queue under `title`. */
export async function call<T>(title: string, answer: Promise<Answer<T>>): Promise<T | undefined> {
  try {
    const result = await answer;
    if (result.status === "ok") return result.data;
    notices.push({ level: "Error", title, text: localizeError(result.error) });
  } catch (err) {
    notices.push({ level: "Error", title, text: String(err) });
  }
  return undefined;
}
