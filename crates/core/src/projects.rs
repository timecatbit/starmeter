//! Which project a request belongs to, by the name the user gave its folder; only that name is kept, never the path.
//!
//! Claude (and Qoder CN, which writes Claude's format) files each conversation under projects/<folder>, the folder
//! written with everything but letters and digits as '-': the folder the conversation was started in, or moved to
//! in the Claude app. That folder is the project, whatever folders the conversation went on to work in (a subfolder,
//! a scratchpad). A worktree Claude makes for a conversation (<repository>/.claude/worktrees/<name>) belongs to its
//! repository, and the Claude app's conversations without a folder (its scratch workspaces) are Claude 对话.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

pub const CLAUDE_CHATS: &str = "Claude 对话";

thread_local! {
    static FOLDERS: RefCell<HashMap<String, Option<String>>> = RefCell::new(HashMap::new());
}

/// Claude's name for a folder: every character but an ASCII letter or digit becomes '-'.
fn encoded(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// The folder a name Claude gave it stands for, looked up on disk: the name alone cannot tell a '-' that was '/'
/// from one that was a space, a dot or a dash. The longest matching name is tried first at each level.
fn decode(name: &str) -> Option<String> {
    let name = name.find("--claude-worktrees-").map_or(name, |at| &name[..at]);
    let bytes = name.as_bytes();
    let (root, rest) = if let Some(rest) = name.strip_prefix('-') {
        ("/".to_string(), rest)
    } else if bytes.len() > 3 && bytes[0].is_ascii_alphabetic() && &name[1..3] == "--" {
        (format!("{}:/", &name[..1]), &name[3..])
    } else {
        return None;
    };
    if rest.is_empty() { None } else { walk(Path::new(&root), rest, 0) }
}

fn walk(folder: &Path, rest: &str, depth: usize) -> Option<String> {
    if depth > 40 {
        return None;
    }
    let mut names: Vec<(String, String)> = std::fs::read_dir(folder)
        .ok()?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .map(|name| (encoded(&name), name))
        .filter(|(code, _)| {
            !code.is_empty()
                && (rest == code || rest.strip_prefix(code.as_str()).is_some_and(|tail| tail.starts_with('-')))
        })
        .collect();
    names.sort_by(|a, b| b.0.chars().count().cmp(&a.0.chars().count()).then_with(|| a.1.cmp(&b.1)));
    for (code, name) in names {
        let path = folder.join(&name);
        if !path.is_dir() {
            continue;
        }
        if code == rest {
            return Some(path.to_string_lossy().into_owned());
        }
        if let Some(found) = walk(&path, &rest[code.len() + 1..], depth + 1) {
            return Some(found);
        }
    }
    None
}

/// The project a Claude conversation folder name stands for.
pub fn folder_project(name: &str) -> Option<String> {
    if let Some(known) = FOLDERS.with(|folders| folders.borrow().get(name).cloned()) {
        return known;
    }
    let found = if name.contains("-Library-Application-Support-Claude-scratch-workspaces-") {
        Some(CLAUDE_CHATS.to_string())
    } else {
        decode(name).and_then(|path| name_of(&path))
    };
    FOLDERS.with(|folders| folders.borrow_mut().insert(name.to_string(), found.clone()));
    found
}

/// The project of a Claude-format transcript, by the folder it is filed under (…/projects/<folder>/…); none when
/// that folder no longer stands for one on disk.
pub fn transcript_project(file: &str) -> Option<String> {
    let parts: Vec<&str> = file.split(['/', '\\']).collect();
    let at = parts.iter().rposition(|part| *part == "projects")?;
    if at + 2 >= parts.len() {
        return None;
    }
    folder_project(parts[at + 1])
}

/// The project of a folder a request ran in: the folder's last name, except a Claude worktree's repository, a Claude
/// scratchpad's conversation folder, and Claude 对话 for the Claude app's scratch workspaces.
pub fn name_of(path: &str) -> Option<String> {
    let path = path.trim().replace('\\', "/");
    let mut path = path.trim_end_matches('/');
    if let Some(at) = path.find("/.claude/worktrees/") {
        path = &path[..at];
    }
    if path.contains("/Library/Application Support/Claude/scratch-workspaces/") {
        return Some(CLAUDE_CHATS.to_string());
    }
    if let Some(folder) = scratchpad(path) {
        return folder_project(folder);
    }
    let last = path.rsplit('/').next().unwrap_or("");
    (!last.is_empty()).then(|| last.to_string())
}

/// /tmp/claude-<uid>/<conversation folder>/…: where Claude keeps a conversation's scratch files.
fn scratchpad(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/private").unwrap_or(path).strip_prefix("/tmp/claude-")?;
    let (uid, rest) = rest.split_once('/')?;
    if uid.is_empty() || !uid.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let folder = rest.split('/').next().filter(|folder| !folder.is_empty())?;
    Some(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_name_their_project() {
        let home = std::env::temp_dir().join(format!("starmeter-projects-{}", crate::runtime::random_hex(8)));
        std::fs::create_dir_all(&home).unwrap();
        let root = home.canonicalize().unwrap();
        for folder in ["my app/src", "my-app.v2", "repo/.claude/worktrees"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        let code = |path: &Path| encoded(&path.to_string_lossy());
        assert_eq!(folder_project(&code(&root.join("my app"))).as_deref(), Some("my app"));
        assert_eq!(folder_project(&code(&root.join("my-app.v2"))).as_deref(), Some("my-app.v2"));
        // A worktree, there or already removed, is its repository's.
        assert_eq!(folder_project(&code(&root.join("repo/.claude/worktrees/fix-a1b2c3"))).as_deref(), Some("repo"));
        assert_eq!(folder_project(&code(&root.join("gone"))), None);
        let file = format!("{}/.claude/projects/{}/abc.jsonl", root.display(), code(&root.join("my app/src")));
        assert_eq!(transcript_project(&file).as_deref(), Some("src"));
        assert_eq!(transcript_project("/x/projects/abc.jsonl"), None);
        let scratch = format!("/private/tmp/claude-501/{}/9f/scratchpad/build", code(&root.join("my app")));
        assert_eq!(name_of(&scratch).as_deref(), Some("my app"));
        assert_eq!(name_of("/r/.claude/worktrees/x/app").as_deref(), Some("r"));
        assert_eq!(
            name_of("/Users/me/Library/Application Support/Claude/scratch-workspaces/a/b/scratch-1").as_deref(),
            Some(CLAUDE_CHATS)
        );
        assert_eq!(name_of("C:\\Users\\me\\proj\\").as_deref(), Some("proj"));
        assert_eq!(name_of(" "), None);
        let _ = std::fs::remove_dir_all(&home);
    }
}
