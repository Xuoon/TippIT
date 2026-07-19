use windows::Win32::System::Diagnostics::Debug::Beep;

/// Blockierender kernel32-Beep auf eigenem Thread (Beep blockiert für die volle Dauer).
pub fn beep(freq: u32, duration_ms: u32) {
    std::thread::spawn(move || {
        let _ = unsafe { Beep(freq, duration_ms) };
    });
}

/// Beep, der den aufrufenden Thread blockiert — für Sequenzen wie Beep → Delay → Tippen.
pub fn beep_blocking(freq: u32, duration_ms: u32) {
    let _ = unsafe { Beep(freq, duration_ms) };
}
