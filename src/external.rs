unsafe extern "C" {
    /// User-provided function that returns the current time in milliseconds.
    pub(crate) fn mwdg_get_time_milliseconds() -> u32;
    /// User-provided function to enter a critical section.
    pub(crate) fn mwdg_enter_critical();
    /// User-provided function to exit a critical section.
    pub(crate) fn mwdg_exit_critical();
}
