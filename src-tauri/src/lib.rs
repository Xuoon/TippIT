mod clipboard;
mod history;
mod hotkeys;
mod sound;
mod state;
mod storage;
mod sync;
mod tray;
mod typing;
mod updater;
mod windows_util;

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::ShortcutState;

use storage::crypto::Secret;
use storage::db;
use storage::index::SearchIndex;
use storage::paths::AppPaths;
use storage::settings::Settings;

fn init_logging(paths: &AppPaths) {
    let file_appender = tracing_appender::rolling::daily(paths.logs_dir(), "tippit.log");
    let (writer, guard) = tracing_appender::non_blocking(file_appender);
    // Guard muss für die Programmlaufzeit leben, sonst gehen Log-Zeilen verloren.
    Box::leak(Box::new(guard));
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .init();
}

/// key.bin laden und eine ggf. abgebrochene Zwei-Phasen-Schlüsselrotation heilen:
/// liegt key.bin.new vor, entscheidet ein Probe-Decrypt, welcher Schlüssel zur DB passt.
fn resolve_secret(paths: &AppPaths, conn: &rusqlite::Connection) -> anyhow::Result<Secret> {
    let secret = Secret::load_or_create(paths)?;
    let Some(pending) = Secret::load_pending(paths).unwrap_or(None) else {
        return Ok(secret);
    };
    let Some((uuid, kind, cipher)) = db::probe_cipher(conn)? else {
        // Leere DB: beide Schlüssel „passen" — konservativ beim alten bleiben.
        Secret::remove_pending(paths);
        return Ok(secret);
    };
    if storage::crypto::decrypt(&secret.derive_keys(), &uuid, kind, &cipher).is_ok() {
        // Rotation kam nie bis zur Umschlüsselung → key.bin.new verwerfen.
        Secret::remove_pending(paths);
        return Ok(secret);
    }
    if storage::crypto::decrypt(&pending.derive_keys(), &uuid, kind, &cipher).is_ok() {
        tracing::warn!("Abgebrochene Schlüsselrotation erkannt — key.bin.new wird übernommen");
        Secret::promote_pending(paths)?;
        // Die Gruppen-Zuordnung ist nach dem Abbruch nicht verlässlich → neu koppeln.
        sync::SyncState::remove(paths);
        return Ok(pending);
    }
    tracing::error!("Weder key.bin noch key.bin.new entschlüsselt die DB — key.bin bleibt aktiv");
    Secret::remove_pending(paths);
    Ok(secret)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Muss das erste Plugin sein: Zweitstart → Callback in der laufenden Instanz, Exit hier.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            windows_util::show_history(app);
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        hotkeys::handle(app, shortcut);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // Schwere Initialisierung bewusst NACH dem Single-Instance-Check.
            let paths = AppPaths::resolve()?;
            init_logging(&paths);
            let settings = Settings::load(&paths);
            let conn = db::open(&paths)?;
            let secret = resolve_secret(&paths, &conn)?;
            let keys = secret.derive_keys();
            let device_id = db::device_id(&conn)?;
            let rows = db::list_active(&conn).unwrap_or_default();
            let index = SearchIndex::build(&rows, &keys);
            tracing::info!(
                "TippIT startet: {} Einträge, Daten unter {:?}",
                rows.len(),
                paths.root
            );
            app.manage(state::AppState::new(
                paths, settings, conn, keys, index, device_id,
            ));
            app.manage(updater::PendingUpdate::default());

            tray::create(app.handle())?;
            hotkeys::register_all(app.handle());
            clipboard::monitor::start(app.handle().clone());
            sync::restart(app.handle());
            updater::check_on_start(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            history::search_history,
            history::entry_thumb,
            history::entry_text,
            history::copy_entry,
            history::type_entry,
            history::pin_entry,
            history::delete_entry,
            history::clear_history,
            history::hide_history_window,
            history::get_settings,
            history::set_settings,
            history::default_settings,
            sync::pairing::sync_status,
            sync::pairing::sync_show_pairing,
            sync::pairing::sync_copy_code,
            sync::pairing::sync_new_code,
            sync::pairing::sync_create_group,
            sync::pairing::sync_join_group,
            sync::pairing::sync_leave_group,
            updater::check_for_update,
            updater::pending_update,
            updater::install_update,
            windows_util::settings_window_ready,
            windows_util::update_window_ready,
            windows_util::close_update_window,
        ])
        .build(tauri::generate_context!())
        .expect("Fehler beim Start von TippIT")
        .run(|_app, event| {
            // App lebt ohne Fenster weiter (Tray-App): Exit nur über "Beenden".
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}
