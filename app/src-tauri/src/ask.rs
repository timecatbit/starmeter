//! The core's questions answered in the panel itself rather than in a dialog: a confirmation, a line of text (a monthly
//! fee, a Key, an address) or one of two choices shows under the row that asked, and the action waits for the answer.
//! With the panel closed (an action from the star's menu) the caller falls back to a system dialog; closing the panel
//! cancels whatever is still waiting.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};

use crate::panel;

fn waiting() -> &'static Mutex<HashMap<u64, Sender<Value>>> {
    static WAITING: OnceLock<Mutex<HashMap<u64, Sender<Value>>>> = OnceLock::new();
    WAITING.get_or_init(|| Mutex::new(HashMap::new()))
}
static NEXT: AtomicU64 = AtomicU64::new(1);

/// Asks the panel and waits for the answer: Some(answer) (null when cancelled), or None when the panel can't ask
/// (closed), so the caller shows its own dialog. Waits at most a quarter of an hour.
pub fn ask(app: &AppHandle, question: Value) -> Option<Value> {
    if !panel::visible(app) {
        return None;
    }
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    let (send, receive) = channel();
    waiting().lock().expect("the question lock").insert(id, send);
    let mut payload = question;
    payload["id"] = json!(id);
    if app.emit("panel-ask", payload).is_err() {
        waiting().lock().expect("the question lock").remove(&id);
        return None;
    }
    let answer = receive.recv_timeout(Duration::from_secs(900)).unwrap_or(Value::Null);
    waiting().lock().expect("the question lock").remove(&id);
    Some(answer)
}

/// A notice (what went wrong, what to fill in instead) shown under the row; nothing waits for it.
pub fn tell(app: &AppHandle, title: &str, detail: &str) -> bool {
    panel::visible(app)
        && app.emit("panel-ask", json!({"kind": "notice", "title": title, "detail": detail, "id": 0})).is_ok()
}

/// Every question still open is answered as cancelled (the panel closed under it).
pub fn cancel_all() {
    for (_, waiting) in waiting().lock().expect("the question lock").drain() {
        let _ = waiting.send(Value::Null);
    }
}

/// The panel's answer: to the action waiting here (a number), or to the SwiftBar plugin's (a string, through a file).
#[tauri::command]
pub fn answer_question(app: AppHandle, id: Value, answer: Value) {
    if let Some(id) = id.as_u64() {
        if let Some(waiting) = waiting().lock().expect("the question lock").remove(&id) {
            let _ = waiting.send(answer);
        }
    } else if let (Some(id), Some(data)) = (id.as_str(), tauri::Manager::try_state::<crate::data::Data>(&app))
        && let crate::data::Data::SwiftBar(files) = &*data
    {
        files.answer(id, answer);
    }
}
