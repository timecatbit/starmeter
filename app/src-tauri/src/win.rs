//! Windows identities the installer and the system know the app by, kept from 5.x's tray: the TokenMonitorTray mutex
//! (setup asks the user to quit the running app while it is held) and the AppUserModelID notifications are filed under.

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::core::w;

/// Held for the app's lifetime; the handle is left open on purpose.
pub fn claim_identity() {
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(w!("timecatbit.TokenMonitor"));
        let mutex: windows::core::Result<HANDLE> = CreateMutexW(None, false, w!("TokenMonitorTray"));
        std::mem::forget(mutex);
    }
}
