use crate::{
    MwdgState, STATE,
    external::{mwdg_enter_critical, mwdg_exit_critical},
};

/// Execute `f` inside the user-provided critical section.
///
/// # Safety
/// - `mwdg_init` must have been called (function pointers must be `Some`).
/// - Must not be called recursively.
#[inline]
pub(crate) fn with_critical_section<R>(f: impl FnOnce(&mut MwdgState) -> R) -> R {
    let state = STATE.as_mut();
    // SAFETY: user should provide the proper implementation of enter critical section function
    unsafe { mwdg_enter_critical() };
    let result = f(state);
    // SAFETY: user should provide the proper implementation of exit critical section function
    unsafe { mwdg_exit_critical() };

    result
}
