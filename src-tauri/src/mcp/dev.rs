//! Инструменты разработчика (только по ключу разработчика, `mcp-dev.json`; Claude в чате
//! студии их не видит): открыть проект, написать в чат студии как пользователь и дождаться
//! ответа, снимок окна без захвата экрана и фокуса, ошибки интерфейса, вкладка. Так внешний
//! Claude проверяет студию сам — тем же путем, что и пользователь.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use super::UiCommand;

pub fn list() -> Value {
    json!([
        { "name": "dev_open_project", "description": "Open a project folder in the studio (as the user would).", "inputSchema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] } },
        { "name": "dev_chat_send", "description": "Send a message to the studio chat as the user (shown in the chat as the user's message).", "inputSchema": { "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] } },
        { "name": "dev_chat_wait", "description": "Wait until the studio Claude finishes its reply (default up to 180 s) and return the turn: assistant text, tool calls, totals.", "inputSchema": { "type": "object", "properties": { "timeout_s": { "type": "integer" } } } },
        { "name": "dev_screenshot", "description": "Capture the studio window (works when covered, does not take focus). Returns a PNG path.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "dev_ui_errors", "description": "Recent UI and core errors (newest first).", "inputSchema": { "type": "object", "properties": { "limit": { "type": "integer" } } } },
        { "name": "dev_eval", "description": "Run JavaScript in the studio window (click a button to check a flow as the user would). No result is returned: look with dev_screenshot.", "inputSchema": { "type": "object", "properties": { "js": { "type": "string" } }, "required": ["js"] } },
        { "name": "dev_tab", "description": "Switch the open tab: home|projects|library|prepare|edit|export.", "inputSchema": { "type": "object", "properties": { "tab": { "type": "string" } }, "required": ["tab"] } },
    ])
}

fn send<E: Event + serde::Serialize + Clone>(app: &AppHandle, event: E) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| event.emit(app)));
}

/// Ход чата после последнего сообщения пользователя — строками.
fn last_turn(dir: &std::path::Path) -> String {
    let raw = std::fs::read_to_string(dir.join(".studio/claude/chat.jsonl")).unwrap_or_default();
    let items: Vec<crate::chat::ChatItem> = raw.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let from = items.iter().rposition(|i| i.role == "user").unwrap_or(0);
    items[from..].iter().map(|i| format!("[{}] {}", i.role, i.text)).collect::<Vec<_>>().join("\n")
}

pub fn call(app: &AppHandle, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "dev_open_project" => {
            let path = args["path"].as_str().ok_or("path required")?;
            let view = crate::project::open(app, path).map_err(|e| format!("{}: {}", e.code, e.message))?;
            send(app, UiCommand::Tab { tab: "prepare".into() });
            Ok(format!("opened {} ({} clips)", view.project.name, view.clips.len()))
        }
        "dev_chat_send" => {
            let text = args["text"].as_str().ok_or("text required")?;
            crate::chat::send(app, text, "user").map_err(|e| format!("{}: {}", e.code, e.message))?;
            send(app, crate::chat::ChatEvent::User { text: text.to_owned() });
            Ok("sent".into())
        }
        "dev_chat_wait" => {
            let limit = Duration::from_secs(args["timeout_s"].as_u64().unwrap_or(180).min(900));
            let started = Instant::now();
            let chat = app.state::<crate::chat::ChatState>();
            while chat.is_busy() {
                if started.elapsed() > limit {
                    return Err("still answering (timeout)".into());
                }
                std::thread::sleep(Duration::from_millis(400));
            }
            let (dir, _) = app.state::<crate::project::ProjectState>().context().ok_or("no project")?;
            Ok(last_turn(&dir))
        }
        "dev_screenshot" => {
            let window = app.get_webview_window("main").ok_or("no main window")?;
            let dir = std::env::temp_dir().join("montage-studio-shots");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let path = dir.join(format!("shot-{}.png", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())));
            capture(&window, &path)?;
            Ok(path.display().to_string())
        }
        "dev_ui_errors" => {
            let limit = args["limit"].as_u64().unwrap_or(20) as usize;
            let list = crate::error::errors_recent();
            Ok(if list.is_empty() { "no errors".into() } else { list.iter().take(limit).map(|e| format!("{} {}: {}", e.at, e.code, e.message)).collect::<Vec<_>>().join("\n---\n") })
        }
        "dev_eval" => {
            let js = args["js"].as_str().ok_or("js required")?;
            app.get_webview_window("main").ok_or("no main window")?.eval(js).map_err(|e| e.to_string())?;
            Ok("ok".into())
        }
        "dev_tab" => {
            let tab = args["tab"].as_str().ok_or("tab required")?;
            send(app, UiCommand::Tab { tab: tab.into() });
            Ok("ok".into())
        }
        _ => Err(format!("unknown dev tool {name}")),
    }
}

/// Снимок окна через `PrintWindow` (содержимое WebView, даже если окно закрыто другими или
/// свернуто не полностью; фокус не меняется).
#[cfg(windows)]
fn capture(window: &tauri::WebviewWindow, path: &PathBuf) -> Result<(), String> {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject};
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

    let hwnd = HWND(window.hwnd().map_err(|e| e.to_string())?.0);
    unsafe {
        let mut r = RECT::default();
        GetClientRect(hwnd, &mut r).map_err(|e| e.to_string())?;
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        if w <= 0 || h <= 0 {
            return Err("window is minimized".into());
        }
        let screen = GetDC(Some(hwnd));
        let dc = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(dc, bmp.into());
        // 2 = PW_RENDERFULLCONTENT (WebView и аппаратная отрисовка), 1 = только клиентская часть.
        let ok = PrintWindow(hwnd, dc, PRINT_WINDOW_FLAGS(3)).as_bool();
        let mut info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(dc, bmp, 0, h as u32, Some(buf.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        SelectObject(dc, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(dc);
        ReleaseDC(Some(hwnd), screen);
        if !ok || lines == 0 {
            return Err("PrintWindow failed".into());
        }
        // BGRA → RGBA.
        for px in buf.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
            px[3] = 255;
        }
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().and_then(|mut wr| wr.write_image_data(&buf)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn capture(_window: &tauri::WebviewWindow, _path: &PathBuf) -> Result<(), String> {
    Err("screenshots are implemented for Windows".into())
}
