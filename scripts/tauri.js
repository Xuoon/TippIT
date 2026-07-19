// Tauri lädt .env.local nicht selbst. Über diesen Bun-Launcher lädt Bun die
// Datei automatisch und reicht die Variablen (u. a. TAURI_SIGNING_PRIVATE_KEY
// für die Updater-Signatur) explizit an den Tauri-Prozess weiter.
const tauri = globalThis.Bun.spawn(
  ["bun", "x", "tauri", ...process.argv.slice(2)],
  {
    env: process.env,
    stderr: "inherit",
    stdin: "inherit",
    stdout: "inherit",
  }
);

process.exit(await tauri.exited);
