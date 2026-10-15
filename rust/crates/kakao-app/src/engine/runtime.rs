//! Native discovery and message waits, injectable for worker transition tests.
use super::SharedFlags;
use std::time::Duration;

pub(super) struct WindowEvent {
    pub hwnd: i64,
    pub age_ms: u32,
    pub follow_up: bool,
}
pub(super) trait WorkerRuntime {
    fn scan(&mut self) -> Option<Vec<i64>>;
    fn alive(&self) -> bool;
    fn watch(&mut self, pids: &[i64]);
    fn install(&mut self, pids: &[i64]) -> bool;
    fn remove(&mut self);
    fn hooked(&self) -> bool;
    fn events(&self) -> Vec<WindowEvent>;
    fn wait(&mut self, flags: &SharedFlags, timeout: Duration);
}

#[derive(Default)]
pub(super) struct NativeRuntime {
    #[cfg(windows)]
    hook: Option<kakao_win32::event_hook::EventHook>,
    #[cfg(windows)]
    watch: kakao_win32::process::PidWatch,
}
impl WorkerRuntime for NativeRuntime {
    fn scan(&mut self) -> Option<Vec<i64>> {
        #[cfg(windows)]
        {
            kakao_win32::process::try_process_ids("kakaotalk.exe").map(|p| p.into_iter().collect())
        }
        #[cfg(not(windows))]
        {
            Some(Vec::new())
        }
    }
    fn alive(&self) -> bool {
        #[cfg(windows)]
        {
            self.watch.all_alive()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
    fn watch(&mut self, _pids: &[i64]) {
        #[cfg(windows)]
        self.watch.watch(_pids);
    }
    fn install(&mut self, _pids: &[i64]) -> bool {
        #[cfg(windows)]
        {
            self.hook = kakao_win32::event_hook::EventHook::install_for_pids(_pids);
        }
        self.hooked()
    }
    fn remove(&mut self) {
        #[cfg(windows)]
        {
            self.hook = None;
        }
    }
    fn hooked(&self) -> bool {
        #[cfg(windows)]
        {
            self.hook.is_some()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
    fn events(&self) -> Vec<WindowEvent> {
        #[cfg(windows)]
        {
            self.hook.as_ref().map_or_else(Vec::new, |hook| {
                let now = unsafe { windows::Win32::System::SystemInformation::GetTickCount() };
                hook.drain()
                    .into_iter()
                    .map(|event| WindowEvent {
                        hwnd: event.hwnd,
                        age_ms: now.wrapping_sub(event.time),
                        follow_up: matches!(
                            event.event,
                            kakao_win32::event_hook::EVENT_CREATE
                                | kakao_win32::event_hook::EVENT_SHOW
                                | kakao_win32::event_hook::EVENT_NAME
                                | kakao_win32::event_hook::EVENT_FOREGROUND
                        ),
                    })
                    .collect()
            })
        }
        #[cfg(not(windows))]
        {
            Vec::new()
        }
    }
    fn wait(&mut self, flags: &SharedFlags, timeout: Duration) {
        #[cfg(windows)]
        if let Some(hook) = self.hook.as_ref() {
            hook.wait_message(timeout);
            return;
        }
        flags.wait_for_control(timeout);
    }
}
