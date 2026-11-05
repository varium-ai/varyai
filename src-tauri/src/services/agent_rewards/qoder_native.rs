//! Read the installed client's current credential without refreshing or rewriting it.
use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};
use security_framework::os::macos::keychain::SecKeychain;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

#[derive(Default)]
pub(super) struct Cache {
    secret: Option<(String, String)>,
}

fn profile(root: &Path, home: &Path) -> Option<(PathBuf, String)> {
    let product: Value =
        serde_json::from_slice(&std::fs::read(root.join("Contents/Resources/product.json")).ok()?)
            .ok()?;
    if product["productId"] != "qoder-cn" {
        return None;
    }
    let directory = product["dataDirectoryName"].as_str()?;
    if !directory.starts_with("com.qodercn.app.")
        || Path::new(directory).components().count() != 1
        || !matches!(
            Path::new(directory).components().next(),
            Some(Component::Normal(_))
        )
    {
        return None;
    }
    let name = product["electronApplicationName"].as_str()?;
    if !name.starts_with("Qoder") || name.len() > 80 || name.chars().any(char::is_control) {
        return None;
    }
    let file = home
        .join("Library/Application Support")
        .join(directory)
        .join("auth.v1.dat");
    file.is_file().then(|| (file, name.to_owned()))
}

fn read_secret(service: &str, account: &str, interactive: bool) -> Option<String> {
    // Legacy Keychain ACL prompts also need the application-level UI guard.
    // This module is the application's only in-process password reader; the
    // cache mutex serializes its reads, and the guard restores the prior state.
    let _silent = if !interactive && SecKeychain::user_interaction_allowed().ok()? {
        Some(SecKeychain::disable_user_interaction().ok()?)
    } else {
        None
    };
    let mut query = ItemSearchOptions::new();
    query
        .class(ItemClass::generic_password())
        .service(service)
        .account(account)
        .load_data(true)
        .skip_authenticated_items(!interactive)
        .limit(1);
    query
        .search()
        .ok()?
        .into_iter()
        .find_map(|item| match item {
            SearchResult::Data(bytes) if !bytes.is_empty() && bytes.len() <= 4096 => {
                String::from_utf8(bytes).ok()
            }
            _ => None,
        })
}

pub(super) fn input(home: &Path, cache: &mut Cache, interactive: bool) -> Option<Value> {
    let (file, name) = [
        PathBuf::from("/Applications/Qoder CN.app"),
        home.join("Applications/Qoder CN.app"),
    ]
    .iter()
    .find_map(|root| profile(root, home))?;
    let service = format!("{name} Safe Storage");
    if interactive
        || cache
            .secret
            .as_ref()
            .is_some_and(|(key, _)| key != &service)
    {
        cache.secret = None;
    }
    if cache.secret.is_none() {
        cache.secret = read_secret(&service, &format!("{name} Key"), interactive)
            .map(|secret| (service, secret));
    }
    Some(json!({"file":file,"password":cache.secret.as_ref().map(|(_, secret)| secret)}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_the_installed_electron_name_and_rejects_unrelated_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("client");
        std::fs::create_dir_all(root.join("Contents/Resources")).unwrap();
        let native = dir
            .path()
            .join("Library/Application Support/com.qodercn.app.stable/auth.v1.dat");
        std::fs::create_dir_all(native.parent().unwrap()).unwrap();
        std::fs::write(&native, b"synthetic-encrypted").unwrap();
        let path = root.join("Contents/Resources/product.json");
        let mut product = json!({"productId":"qoder-cn","dataDirectoryName":"com.qodercn.app.stable","electronApplicationName":"Qoder CN App"});
        std::fs::write(&path, product.to_string()).unwrap();
        assert_eq!(
            profile(&root, dir.path()),
            Some((native, "Qoder CN App".into()))
        );
        product["dataDirectoryName"] = json!("com.qodercn.app.stable/../other");
        std::fs::write(&path, product.to_string()).unwrap();
        assert!(profile(&root, dir.path()).is_none());
        product["dataDirectoryName"] = json!("com.qodercn.app.stable");
        product["productId"] = json!("unrelated");
        std::fs::write(&path, product.to_string()).unwrap();
        assert!(profile(&root, dir.path()).is_none());
    }
}
