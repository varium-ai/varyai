use super::{error, AppError};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
#[cfg(windows)]
use std::os::windows::fs::FileTypeExt;
use std::path::{Path, PathBuf};

enum Node {
    Missing,
    File(Vec<u8>),
    Directory(Vec<(String, Node)>),
    Link(PathBuf),
}
pub struct Tree {
    path: PathBuf,
    node: Node,
}
impl Tree {
    pub fn capture(path: PathBuf) -> Result<Self, AppError> {
        let mut size = 0;
        let mut entries = 0;
        Ok(Self {
            node: read(&path, &mut size, &mut entries)?,
            path,
        })
    }
    pub fn fingerprint(&self) -> Value {
        json!([self.path, digest(&self.node)])
    }
    pub fn restore(&self) -> Result<(), AppError> {
        restore(&self.path, &self.node)
    }
}
fn read(path: &Path, size: &mut usize, entries: &mut usize) -> Result<Node, AppError> {
    *entries += 1;
    if *entries > 20_000 {
        return Err(error("snapshotTooLarge"));
    }
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Node::Missing),
        Err(_) => return Err(error("readFailed")),
    };
    if meta.file_type().is_symlink() {
        return Ok(Node::Link(
            std::fs::read_link(path).map_err(|_| error("readFailed"))?,
        ));
    }
    if meta.is_dir() {
        let mut children = Vec::new();
        for child in std::fs::read_dir(path).map_err(|_| error("readFailed"))? {
            let child = child.map_err(|_| error("readFailed"))?;
            children.push((
                child
                    .file_name()
                    .into_string()
                    .map_err(|_| error("invalidConfig"))?,
                read(&child.path(), size, entries)?,
            ));
        }
        children.sort_by(|a, b| a.0.cmp(&b.0));
        return Ok(Node::Directory(children));
    }
    if !meta.is_file() {
        return Err(error("invalidConfig"));
    }
    *size = size.saturating_add(meta.len() as usize);
    if *size > 64 * 1024 * 1024 {
        return Err(error("snapshotTooLarge"));
    }
    Ok(Node::File(
        std::fs::read(path).map_err(|_| error("readFailed"))?,
    ))
}
fn digest(node: &Node) -> Value {
    match node {
        Node::Missing => Value::Null,
        Node::File(b) => json!(["file", format!("{:x}", Sha256::digest(b))]),
        Node::Link(p) => json!(["link", p]),
        Node::Directory(children) => json!(children
            .iter()
            .map(|(name, n)| json!([name, digest(n)]))
            .collect::<Vec<_>>()),
    }
}
fn remove(path: &Path) -> Result<(), AppError> {
    match std::fs::symlink_metadata(path) {
        #[cfg(windows)]
        Ok(m) if m.file_type().is_symlink_dir() => {
            // Remove the link itself, including when its target no longer exists.
            std::fs::remove_dir(path).map_err(|_| error("restoreFailed"))
        }
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {
            std::fs::remove_dir_all(path).map_err(|_| error("restoreFailed"))
        }
        Ok(_) => std::fs::remove_file(path).map_err(|_| error("restoreFailed")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(error("restoreFailed")),
    }
}

fn restore(path: &Path, node: &Node) -> Result<(), AppError> {
    if let Node::Directory(children) = node {
        if std::fs::symlink_metadata(path).is_ok_and(|m| !m.is_dir() || m.file_type().is_symlink())
        {
            remove(path)?;
        }
        std::fs::create_dir_all(path).map_err(|_| error("restoreFailed"))?;
        for child in std::fs::read_dir(path).map_err(|_| error("restoreFailed"))? {
            let child = child.map_err(|_| error("restoreFailed"))?;
            if !children
                .iter()
                .any(|(name, _)| child.file_name() == name.as_str())
            {
                remove(&child.path())?;
            }
        }
        for (name, n) in children {
            restore(&path.join(name), n)?;
        }
        return Ok(());
    }
    remove(path)?;
    match node {
        Node::File(bytes) => crate::config::atomic_write_private(path, bytes),
        Node::Link(target) => {
            if let Some(p) = path.parent() {
                std::fs::create_dir_all(p).map_err(|_| error("restoreFailed"))?;
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(target, path).map_err(|_| error("restoreFailed"))
            }
            #[cfg(windows)]
            {
                if target.is_dir() {
                    std::os::windows::fs::symlink_dir(target, path)
                } else {
                    std::os::windows::fs::symlink_file(target, path)
                }
                .map_err(|_| error("restoreFailed"))
            }
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symlink(target: &Path, path: &Path, directory: bool) {
        #[cfg(unix)]
        {
            let _ = directory;
            std::os::unix::fs::symlink(target, path).unwrap();
        }
        #[cfg(windows)]
        if directory {
            std::os::windows::fs::symlink_dir(target, path).unwrap();
        } else {
            std::os::windows::fs::symlink_file(target, path).unwrap();
        }
    }

    #[test]
    fn restore_removes_added_links_without_removing_their_targets() {
        for directory in [false, true] {
            for dangling in [false, true] {
                for nested in [false, true] {
                    let temp = tempfile::tempdir().unwrap();
                    let target = temp.path().join("target");
                    let content = if directory {
                        std::fs::create_dir(&target).unwrap();
                        target.join("SKILL.md")
                    } else {
                        target.clone()
                    };
                    std::fs::write(&content, "Preserve this content").unwrap();
                    let root = temp.path().join("snapshot");
                    let link = if nested {
                        std::fs::create_dir(&root).unwrap();
                        root.join("link")
                    } else {
                        root.clone()
                    };
                    let before = Tree::capture(root.clone()).unwrap();
                    symlink(&target, &link, directory);
                    let remaining = if dangling {
                        let moved = temp.path().join("moved-target");
                        std::fs::rename(&target, &moved).unwrap();
                        if directory {
                            moved.join("SKILL.md")
                        } else {
                            moved
                        }
                    } else {
                        content
                    };

                    before.restore().unwrap();

                    assert_eq!(
                        std::fs::symlink_metadata(&link).unwrap_err().kind(),
                        std::io::ErrorKind::NotFound
                    );
                    assert_eq!(
                        std::fs::read_to_string(remaining).unwrap(),
                        "Preserve this content"
                    );
                    assert_eq!(
                        Tree::capture(root).unwrap().fingerprint(),
                        before.fingerprint()
                    );
                }
            }
        }
    }
}
