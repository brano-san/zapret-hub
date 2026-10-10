import { mount } from "svelte";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App.svelte";
import "./app.css";

// The system's theme until the settings arrive: the first frame matches the desktop.
document.documentElement.dataset.theme = matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";

const app = mount(App, { target: document.getElementById("app")! });
// The window is made hidden; shown once the page has drawn, so it never flashes white.
void getCurrentWindow().show();

export default app;
