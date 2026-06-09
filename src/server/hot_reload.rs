use notify::{Event, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;

use super::cache::CacheManager;

#[derive(Debug, Clone)]
pub enum FileChangeEvent {
    StyleChanged { path: PathBuf },
    DataChanged { path: PathBuf },
}

pub struct HotReloader {
    #[allow(dead_code)]
    tx: broadcast::Sender<FileChangeEvent>,
}

impl HotReloader {
    pub fn start(watch_paths: Vec<PathBuf>, cache: Arc<CacheManager>) -> Self {
        let (tx, _) = broadcast::channel::<FileChangeEvent>(256);
        let tx2 = tx.clone();

        std::thread::spawn(move || {
            let mut watcher =
                notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
                    if let Ok(event) = res {
                        for path in &event.paths {
                            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                            let event = match ext {
                                "toml" => FileChangeEvent::StyleChanged { path: path.clone() },
                                "geojson" | "json" | "tif" | "tiff" => {
                                    FileChangeEvent::DataChanged { path: path.clone() }
                                },
                                _ => continue,
                            };
                            let _ = tx2.send(event);
                        }
                    }
                })
                .unwrap();

            for path in &watch_paths {
                let _ = watcher.watch(path, RecursiveMode::Recursive);
            }

            std::thread::park();
        });

        // Spawn a task to handle cache invalidation
        let mut rx = tx.subscribe();
        let cache_clone = cache;
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(FileChangeEvent::StyleChanged { path }) => {
                        let key = path.to_string_lossy().to_string();
                        cache_clone.invalidate_style(&key).await;
                        tracing::info!("Hot-reloaded style: {}", key);
                    },
                    Ok(FileChangeEvent::DataChanged { path }) => {
                        let key = path.to_string_lossy().to_string();
                        cache_clone.invalidate_layer(&key).await;
                        tracing::info!("Hot-reloaded data: {}", key);
                    },
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Hot reload lagged by {} events", n);
                    },
                    Err(_) => break,
                }
            }
        });

        Self { tx }
    }
}
