import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Single-origin is the target: `ferritin serve --features dev-proxy` serves this
// app and the API together. This proxy is only for running Vite standalone
// against a separate `ferritin serve` (port 8080); it forwards the /api mount and
// otherwise falls through to Vite for the SPA. Harmless under dev-proxy.
export default defineConfig({
  plugins: [react()],
});
