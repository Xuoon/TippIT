/// Beep auf eigenem Thread (die Plattform-Impl blockiert für die volle Dauer).
pub fn beep(freq: u32, duration_ms: u32) {
    std::thread::spawn(move || crate::platform::beep_blocking(freq, duration_ms));
}

/// Beep, der den aufrufenden Thread blockiert — für Sequenzen wie Beep → Delay → Tippen.
pub fn beep_blocking(freq: u32, duration_ms: u32) {
    crate::platform::beep_blocking(freq, duration_ms);
}
