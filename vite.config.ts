import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
    // prevent vite from obscuring rust errors
    clearScreen: false,

    // Tauri expects a fixed port, fail if that port is not available
    server: {
        strictPort: true,
        watch: {
            ignored: ["**/src-tauri/target/**"],
        },
    },

    // to make use of `TAURI_PLATFORM`, `TAURI_ARCH`, `TAURI_FAMILY`,
    // `TAURI_PLATFORM_VERSION`, `TAURI_PLATFORM_TYPE` and `TAURI_DEBUG`
    envPrefix: ["VITE_", "TAURI_"],

    build: {
        // Tauri supports es2021
        target: ["es2021", "chrome100", "safari13"],
        minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
        sourcemap: !!process.env.TAURI_DEBUG,
        rollupOptions: {
            input: {
                overlay: "./src/overlay/overlay.html",
                window: "./src/window/window.html",
            },
        },
    },

    plugins: [svelte()],
});