import "../core/global.css";
import "./window.css";
import ProfilesWindow from "./profiles/ProfilesWindow.svelte";
import PreferencesWindow from "./preferences/PreferencesWindow.svelte";
import DetailsWindow from "./details/DetailsWindow.svelte";
import { appWindow } from "@tauri-apps/api/window";
import type { TauriEvent, Preferences } from "../core/types";
import { getPreferences } from "../core/ipc";

window.addEventListener("DOMContentLoaded", () => {
    appWindow.show();
    appWindow.setFocus();
});

document.addEventListener("contextmenu", (e) => e.preventDefault());


function applyTheme(p: Preferences) {
    const root = document.documentElement;
    root.style.setProperty("--app-gradient-1", p.appGradient1);
    root.style.setProperty("--app-gradient-2", p.appGradient2);
    root.style.setProperty("--primary-highlight", p.appGradient1);
    root.style.setProperty("--secondary-highlight", p.appGradient2);
}

getPreferences().then(applyTheme);
appWindow.listen("preferences_update", (event: TauriEvent<Preferences>) => {
    applyTheme(event.payload);
});


document.querySelector("#exit-button").addEventListener("click", () => appWindow.close());
document.querySelector("#minimize-button").addEventListener("click", () => appWindow.minimize());

const target = document.querySelector("#content");

const app = getWindowType();

function getWindowType() {
    let windowType = window.location.hash.split("#")[1];
    switch (windowType) {
        case "preferences":
            return new PreferencesWindow({
                target
            });
        case "profiles":
            return new ProfilesWindow({
                target
            });
        case "details":
            return new DetailsWindow({
                target
            });
        default:
            appWindow.close();
            break;
    }
}

export default app;
