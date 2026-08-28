<script lang="ts">
    import { appWindow } from "@tauri-apps/api/window";
    import LineButton from "../widgets/LineButton.svelte";
    import StyledCheckbox from "./StyledCheckbox.svelte";
    import type { Preferences } from "../../core/types";
    import * as ipc from "../../core/ipc";

    let preferences: Preferences;
    let error: string;

    function init() {
        ipc.getPreferences().then((p: Preferences) => (preferences = p));
    }

    function confirm() {
        ipc.setPreferences(preferences)
            .then(() => appWindow.close())
            .catch((e) => {
                error = e.message ?? e;
                appWindow.show();
            });

        appWindow.hide();
    }

    function applyTheme(p: Preferences) {
        const root = document.documentElement;
        root.style.setProperty("--app-gradient-1", p.appGradient1);
        root.style.setProperty("--app-gradient-2", p.appGradient2);
        root.style.setProperty("--primary-highlight", p.appGradient1);
        root.style.setProperty("--secondary-highlight", p.appGradient2);
    }

    $: if (preferences) {
        applyTheme(preferences);
    }

    
    function applyThemePreset(preset: string) {
        switch (preset) {
            case "swift":
                preferences.appGradient1 = "#8e24aa";
                preferences.appGradient2 = "#3f1d4f";
                preferences.dailyColor = "#a9c6ff";
                preferences.weeklyColor = "#aa8bf3";
                preferences.spotifyAccentColor = "#1ed760";
                preferences.overlayTextColor = "#f5f7fb";
                break;
            case "ice":
                preferences.appGradient1 = "#071a2b";
                preferences.appGradient2 = "#123a5a";
                preferences.dailyColor = "#8eeaff";
                preferences.weeklyColor = "#79a9ff";
                preferences.spotifyAccentColor = "#58e6d9";
                preferences.overlayTextColor = "#f1fbff";
                break;
            case "void":
                preferences.appGradient1 = "#100b1f";
                preferences.appGradient2 = "#2b1457";
                preferences.dailyColor = "#c3a6ff";
                preferences.weeklyColor = "#8b5cf6";
                preferences.spotifyAccentColor = "#b86cff";
                preferences.overlayTextColor = "#f7f1ff";
                break;
            case "crimson":
                preferences.appGradient1 = "#140708";
                preferences.appGradient2 = "#4a0d14";
                preferences.dailyColor = "#ffb0b0";
                preferences.weeklyColor = "#ff5b69";
                preferences.spotifyAccentColor = "#ff3347";
                preferences.overlayTextColor = "#fff4f4";
                break;
            case "emerald":
                preferences.appGradient1 = "#071713";
                preferences.appGradient2 = "#0d4937";
                preferences.dailyColor = "#9cf5d3";
                preferences.weeklyColor = "#4fd1a1";
                preferences.spotifyAccentColor = "#21d98b";
                preferences.overlayTextColor = "#effff9";
                break;
            case "monochrome":
                preferences.appGradient1 = "#111111";
                preferences.appGradient2 = "#303030";
                preferences.dailyColor = "#f0f0f0";
                preferences.weeklyColor = "#bdbdbd";
                preferences.spotifyAccentColor = "#ffffff";
                preferences.overlayTextColor = "#f7f7f7";
                break;
        }
    }

    function resetColors() {
        preferences.appGradient1 = "#8e24aa";
        preferences.appGradient2 = "#3f1d4f";
        preferences.dailyColor = "#a9c6ff";
        preferences.weeklyColor = "#aa8bf3";
        preferences.spotifyAccentColor = "#1ed760";
        preferences.overlayTextColor = "#f5f7fb";
        preferences.overlayScale = 100;
        preferences.overlayOpacity = 100;
        preferences.displayTimer = true;
        preferences.displayDailyClears = true;
        preferences.displayWeeklyClears = true;
        preferences.displaySpotify = true;
        preferences.overlayPosition = "top-right";
        preferences.overlayOffsetX = 22;
        preferences.overlayOffsetY = 18;
    }

    init();
</script>

<main>
    <div class="title-section">
        <h1>Preferences</h1>
        <div class="volc-credit">
            by <span>Volc &lt;3</span>
        </div>
    </div>

    {#if preferences}
        <div class="preferences">
            {#if error}
                <p class="error">{error}</p>
            {/if}

            <div class="preference">
                <StyledCheckbox bind:checked={preferences.enableOverlay}>
                    Enable overlay
                </StyledCheckbox>
            </div>

            <div class="preference-group">
                <div class="preference">
                    <StyledCheckbox
                        bind:checked={preferences.displayDailyClears}
                        disabled={!preferences.enableOverlay}
                    >
                        Display daily clears
                    </StyledCheckbox>
                </div>

                <div class="preference">
                    <StyledCheckbox
                        bind:checked={preferences.displayClearNotifications}
                        disabled={!preferences.enableOverlay}
                    >
                        Display activity clear notifications
                    </StyledCheckbox>
                </div>

                <div class="preference">
                    <StyledCheckbox
                        bind:checked={preferences.displayMilliseconds}
                        disabled={!preferences.enableOverlay}
                    >
                        Display timer milliseconds
                    </StyledCheckbox>
                </div>
            </div>

            <div class="spotify-section">
                <h2>Spotify</h2>

                <label for="spotify-client-id">
                    Spotify Client ID
                </label>

                <input
                    id="spotify-client-id"
                    type="text"
                    bind:value={preferences.spotifyClientId}
                    placeholder="Paste your Spotify Client ID"
                    autocomplete="off"
                    spellcheck="false"
                />

                <p class="helper">
                    Create your own Spotify developer app, add
                    http://127.0.0.1:8975/callback as the redirect URI,
                    then paste the Client ID here.
                </p>
            </div>

            <div class="appearance-section">
                <h2>Appearance</h2>

                <div class="preset-row">
                    <label for="theme-preset">Preset Theme</label>
                    <select id="theme-preset" on:change={(e) => applyThemePreset(e.currentTarget.value)}>
                        <option value="" selected disabled>Choose a preset...</option>
                        <option value="swift">Swift Default</option>
                        <option value="ice">Ice</option>
                        <option value="void">Void</option>
                        <option value="crimson">Crimson</option>
                        <option value="emerald">Emerald</option>
                        <option value="monochrome">Monochrome</option>
                    </select>
                </div>

                {#each [
                    ["App gradient 1", "appGradient1"],
                    ["App gradient 2", "appGradient2"],
                    ["Daily clears color", "dailyColor"],
                    ["Weekly clears color", "weeklyColor"],
                    ["Spotify accent color", "spotifyAccentColor"],
                    ["Overlay text color", "overlayTextColor"]
                ] as item}
                    <div class="color-row">
                        <label>{item[0]}</label>
                        <input type="color" bind:value={preferences[item[1]]} />
                        <span>{preferences[item[1]]}</span>
                    </div>
                {/each}
                <button class="reset-colors" type="button" on:click={resetColors}>↶ Reset to default colors</button>
            </div>


            <div class="overlay-customization-section">
                <h2>Overlay</h2>

                <div class="section-toggle-grid">
                    <label><input type="checkbox" bind:checked={preferences.displayTimer} /> <span>Timer</span></label>
                    <label><input type="checkbox" bind:checked={preferences.displayDailyClears} /> <span>Daily Clears</span></label>
                    <label><input type="checkbox" bind:checked={preferences.displayWeeklyClears} /> <span>Weekly Clears</span></label>
                    <label><input type="checkbox" bind:checked={preferences.displaySpotify} /> <span>Spotify</span></label>
                </div>

                <div class="slider-row">
                    <div class="slider-heading"><label for="overlay-scale">Scale</label><span>{preferences.overlayScale}%</span></div>
                    <input id="overlay-scale" type="range" min="75" max="150" step="5" bind:value={preferences.overlayScale} />
                </div>
                <div class="slider-row">
                    <div class="slider-heading"><label for="overlay-opacity">Opacity</label><span>{preferences.overlayOpacity}%</span></div>
                    <input id="overlay-opacity" type="range" min="20" max="100" step="5" bind:value={preferences.overlayOpacity} />
                </div>

                <div class="position-row">
                    <label for="overlay-position">Position</label>
                    <select id="overlay-position" bind:value={preferences.overlayPosition}>
                        <option value="top-left">Top Left</option>
                        <option value="top-right">Top Right</option>
                        <option value="bottom-left">Bottom Left</option>
                        <option value="bottom-right">Bottom Right</option>
                    </select>
                </div>

                <div class="offset-grid">
                    <label for="overlay-offset-x">
                        <span>X Offset</span>
                        <input id="overlay-offset-x" type="number" min="0" max="500" step="1" bind:value={preferences.overlayOffsetX} />
                    </label>
                    <label for="overlay-offset-y">
                        <span>Y Offset</span>
                        <input id="overlay-offset-y" type="number" min="0" max="500" step="1" bind:value={preferences.overlayOffsetY} />
                    </label>
                </div>
                <p class="helper">Scale changes the overlay size. Opacity controls transparency. Position and offsets control where the overlay sits on screen.</p>
            </div>

            <div class="actions">
                <LineButton clickCallback={confirm}>Confirm</LineButton>
            </div>
        </div>
    {/if}
</main>

<style>
    .title-section {
        margin: 24px 48px 18px 48px;
    }

    h1 {
        margin: 0;
    }

    .volc-credit {
        margin-top: 2px;
        font-size: 14px;
        font-weight: 400;
        color: rgba(255, 255, 255, 0.5);
    }

    .volc-credit span {
        color: #a77bff;
        font-weight: 600;
    }

    h2 {
        margin: 0 0 12px 0;
        font-size: 18px;
        font-weight: 500;
    }

    .preferences {
        margin: 16px 48px;
    }

    .preference-group {
        padding: 8px 12px;
        border: 1px solid rgba(255, 255, 255, 0.1);
    }

    .preference {
        margin: 12px 8px;
    }

    .spotify-section {
        margin-top: 20px;
        padding: 16px 20px;
        border: 1px solid rgba(255, 255, 255, 0.1);
    }

    .spotify-section label {
        display: block;
        margin-bottom: 8px;
        font-size: 14px;
    }

    .spotify-section input {
        box-sizing: border-box;
        width: 100%;
        padding: 10px 12px;
        color: white;
        background: rgba(255, 255, 255, 0.06);
        border: 1px solid rgba(255, 255, 255, 0.15);
        border-radius: 4px;
        outline: none;
        font-family: inherit;
        font-size: 14px;
    }

    .spotify-section input:focus {
        border-color: rgba(255, 255, 255, 0.4);
    }

    .helper {
        margin: 8px 0 0 0;
        color: rgba(255, 255, 255, 0.55);
        font-size: 11px;
        line-height: 1.4;
    }

    .error {
        color: var(--error);
    }

    .appearance-section { margin-top:20px; padding:16px 20px; border:1px solid rgba(255,255,255,.1); }
    .color-row { display:grid; grid-template-columns:1fr 44px 88px; align-items:center; gap:10px; margin:10px 0; }
    .color-row label { font-size:14px; }
    .color-row input[type="color"] { width:38px; height:30px; padding:2px; border:1px solid rgba(255,255,255,.15); border-radius:4px; background:rgba(255,255,255,.06); cursor:pointer; }
    .color-row span { font-size:12px; color:rgba(255,255,255,.65); font-family:monospace; }
    .reset-colors { margin-top:10px; padding:8px 12px; color:#c99cff; background:transparent; border:1px solid rgba(201,156,255,.65); border-radius:4px; font-family:inherit; cursor:pointer; }

    .actions {
        margin-top: 24px;
        float: right;
    }

    .overlay-customization-section { margin-top: 20px; padding: 16px 20px; border: 1px solid rgba(255,255,255,.1); }
    .slider-row { margin-top: 14px; }
    .slider-heading { display:flex; justify-content:space-between; align-items:center; margin-bottom:7px; font-size:14px; }
    .slider-heading span { color:rgba(255,255,255,.65); font-variant-numeric:tabular-nums; }
    .slider-row input[type="range"] { width:100%; margin:0; accent-color:var(--primary-highlight-lighter); }


    .position-row { display:flex; justify-content:space-between; align-items:center; gap:16px; margin-top:18px; }
    .position-row select { min-width:150px; }
    .offset-grid { display:grid; grid-template-columns:1fr 1fr; gap:12px; margin-top:14px; }
    .offset-grid label { display:flex; align-items:center; justify-content:space-between; gap:10px; font-size:14px; }
    .offset-grid input { width:72px; }


    .section-toggle-grid { display:grid; grid-template-columns:1fr 1fr; gap:10px 18px; margin:12px 0 18px; }
    .section-toggle-grid label { display:flex; align-items:center; gap:8px; font-size:14px; cursor:pointer; }
    .section-toggle-grid input { margin:0; }


    .preset-row {
        display: flex;
        justify-content: space-between;
        align-items: center;
        gap: 16px;
        margin: 12px 0 18px;
    }

    .preset-row select {
        min-width: 170px;
    }

</style>