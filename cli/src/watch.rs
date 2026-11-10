use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, SystemTime};

const INTERVAL: Duration = Duration::from_millis(400);

type Signature = Vec<(PathBuf, Option<SystemTime>, u64)>;

pub fn spawn<T: Send + 'static>(root: PathBuf, tx: Sender<T>, message: fn() -> T) {
    thread::spawn(move || {
        let mut last = signature(&root);
        loop {
            thread::sleep(INTERVAL);
            let now = signature(&root);
            if now != last {
                last = now;
                if tx.send(message()).is_err() {
                    return;
                }
            }
        }
    });
}

fn signature(root: &Path) -> Signature {
    let mut paths = vec![root.join("all-todos.json")];
    if let Ok(entries) = fs::read_dir(root.join("projects")) {
        paths.extend(
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "json")),
        );
    }
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let meta = fs::metadata(&p).ok();
            let modified = meta.as_ref().and_then(|m| m.modified().ok());
            let len = meta.map_or(0, |m| m.len());
            (p, modified, len)
        })
        .collect()
}
