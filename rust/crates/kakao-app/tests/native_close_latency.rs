#![cfg(windows)]
//! Only test-owned, off-screen windows are mutated. No KakaoTalk is required.
use kakao_app::config::AppSettings;
use kakao_app::engine::{EngineCaches, SharedFlags, apply_evaluation};
use kakao_core::Evaluation;
use kakao_win32::{RealWin32, Win32Api};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

static CLOSE_VISIBILITY: AtomicU32 = AtomicU32::new(0);

#[test]
fn nested_popup_child_keeps_offset_after_zero_size_and_restore() {
    // Only windows owned by this test are targeted. STATIC uses the OS class.
    unsafe {
        let parent = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("audit-parent"),
            WS_POPUP | WS_VISIBLE,
            -32000,
            -32000,
            300,
            200,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        struct Cleanup(HWND);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = unsafe { DestroyWindow(self.0) };
            }
        }
        let _cleanup = Cleanup(parent);
        let wrapper = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("wrapper"),
            WS_CHILD | WS_VISIBLE,
            10,
            20,
            200,
            150,
            Some(parent),
            None,
            None,
            None,
        )
        .unwrap();
        let child = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("child"),
            WS_CHILD | WS_VISIBLE,
            25,
            35,
            120,
            80,
            Some(wrapper),
            None,
            None,
            None,
        )
        .unwrap();
        let child_id = child.0 as isize as i64;
        let parent_id = parent.0 as isize as i64;
        let api = RealWin32::new();
        let before = api.get_window_rect(child_id).unwrap();
        let pid = i64::from(std::process::id());
        let graph = kakao_app::graph_build::build_graph(&api, &[pid]);
        let mut evaluation = Evaluation::default();
        evaluation.actions.hide = vec![parent_id, child_id];
        evaluation.actions.set_pos = vec![vec![parent_id, 0, 0, 0, 0], vec![child_id, 0, 0, 0, 0]];
        let flags = SharedFlags::from_settings(&AppSettings::default(), true);
        let mut caches = EngineCaches::new();
        apply_evaluation(
            &api,
            &graph,
            &evaluation,
            &mut caches.snapshots,
            &HashSet::from([pid]),
            &flags,
        );
        assert_eq!(api.get_window_rect(child_id).unwrap().width(), 0);
        assert_eq!(caches.drain_restore_all(&api), (0, String::new()));
        assert_eq!(api.get_window_rect(child_id), Some(before));
        assert!(api.has_visible_style(child_id));
        assert!(caches.snapshots.is_empty());
    }
}
unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CLOSE => {
            CLOSE_VISIBILITY.store(
                if unsafe { IsWindowVisible(hwnd) }.as_bool() {
                    1
                } else {
                    2
                },
                Ordering::SeqCst,
            );
            std::thread::sleep(Duration::from_millis(300));
            LRESULT(0) // Deliberately refuse close after the app's timeout.
        }
        WM_APP => {
            let _ = unsafe { DestroyWindow(hwnd) };
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

#[test]
fn slow_native_close_observes_window_already_hidden() {
    let (tx, rx) = mpsc::channel();
    let owner = std::thread::spawn(move || unsafe {
        let module = GetModuleHandleW(None).unwrap();
        let class = w!("KakaoAuditSlowClose");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        };
        assert_ne!(RegisterClassW(&wc), 0);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("test-owned"),
            WS_POPUP | WS_VISIBLE,
            -32000,
            -32000,
            10,
            10,
            None,
            None,
            Some(module.into()),
            None,
        )
        .unwrap();
        tx.send(hwnd.0 as isize as i64).unwrap();
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            DispatchMessageW(&msg);
        }
    });
    let hwnd = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let api = RealWin32::new();
    let pid = i64::from(std::process::id());
    let graph = kakao_app::graph_build::build_graph(&api, &[pid]);
    let mut evaluation = Evaluation::default();
    evaluation.actions.hide.push(hwnd);
    evaluation.actions.close.push(hwnd);
    let flags = SharedFlags::from_settings(&AppSettings::default(), true);
    let mut caches = EngineCaches::new();
    apply_evaluation(
        &api,
        &graph,
        &evaluation,
        &mut caches.snapshots,
        &HashSet::from([pid]),
        &flags,
    );
    let hidden = !api.is_window_visible(hwnd);
    unsafe {
        PostMessageW(
            Some(HWND(hwnd as isize as *mut _)),
            WM_APP,
            WPARAM(0),
            LPARAM(0),
        )
        .unwrap();
    }
    owner.join().unwrap();
    assert!(hidden);
    assert_eq!(
        CLOSE_VISIBILITY.load(Ordering::SeqCst),
        2,
        "slow WM_CLOSE must start after hide"
    );
    assert!(caches.snapshots.values().any(|s| s.was_visible));
}
