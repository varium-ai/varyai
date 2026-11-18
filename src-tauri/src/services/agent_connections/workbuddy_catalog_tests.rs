use super::*;

#[test]
fn workbuddy_native_catalog_additions_survive_reload_and_disconnect() -> Result<(), String> {
    for app in ["workbuddy", "workbuddy-ai"] {
        for array in [true, false] {
            let dir = tempfile::tempdir().map_err(|_| FAILED)?;
            let manager = Manager {
                root: dir.path().join("receipts"),
            };
            let path = dir.path().join("models.json");
            let native = json!({"id":"native","apiKey":"synthetic-native"});
            let archived = json!({"id":"managed","apiKey":"synthetic-original"});
            let added = json!({"id":"new-native","apiKey":"synthetic-added"});
            let managed = json!({"id":"managed","apiKey":"synthetic-vary","url":"http://127.0.0.1:1/v1/chat/completions"});
            let document = |rows: Value| {
                if array {
                    rows
                } else {
                    json!({"models":rows,"custom":true})
                }
            };
            fs::write(&path, document(json!([native, archived])).to_string())
                .map_err(|_| FAILED)?;
            let file = FileEdit::prepare_document(path.clone(), true, |value| {
                *value = document(json!([native, managed]));
                Ok(())
            })?;
            manager.install(Receipt {
                app: app.into(),
                token: "synthetic-vary".into(),
                model: "managed".into(),
                model_map: BTreeMap::new(),
                visible_models: None,
                files: vec![file],
                suspended: false,
                applied_at_ms: Some(2000),
            })?;
            // Simulate the client publishing another native model between gateway runs.
            let live = document(json!([managed, added, native]));
            fs::write(&path, live.to_string()).map_err(|_| FAILED)?;
            let reopened = Manager {
                root: manager.root.clone(),
            };
            let status = reopened.status_with_desktop(app, |_, applied| {
                assert_eq!(applied, Some(2000));
                Some(desktop::State::Current)
            })?;
            assert!(
                status.configured && !status.recovery_pending,
                "{app} array={array}"
            );
            assert_eq!(read(&path)?.as_deref(), Some(live.to_string().as_str()));
            let loaded = reopened.load(app)?.ok_or(FAILED)?;
            let mut reordered = loaded.clone();
            let value = &mut reordered.files[0].installed;
            let rows = if array { value } else { &mut value["models"] };
            rows.as_array_mut().ok_or(FAILED)?.reverse();
            assert!(!startup::changed(&loaded, &reordered));
            // A genuine change to any existing model must still require recovery.
            for field in ["url", "apiKey", "id"] {
                let mut changed = live.clone();
                let rows = if array {
                    &mut changed
                } else {
                    &mut changed["models"]
                };
                rows[0][field] = json!("synthetic-changed");
                fs::write(&path, changed.to_string()).map_err(|_| FAILED)?;
                assert!(
                    reopened
                        .status_with_desktop(app, |_, _| None)?
                        .recovery_pending
                );
            }
            for invalid_rows in [
                json!([managed, managed, added, native]),
                json!([added, native]),
                json!([managed,added,native,{"id":"unexpected-managed","apiKey":"synthetic-vary"}]),
            ] {
                fs::write(&path, document(invalid_rows).to_string()).map_err(|_| FAILED)?;
                assert!(
                    reopened
                        .status_with_desktop(app, |_, _| None)?
                        .recovery_pending
                );
            }
            fs::write(&path, live.to_string()).map_err(|_| FAILED)?;
            if array {
                reopened.suspend(app)?;
                assert!(reopened.load(app)?.is_some_and(|receipt| receipt.suspended));
            }
            reopened.disconnect(app)?;
            let restored: Value =
                serde_json::from_str(&read(&path)?.ok_or(FAILED)?).map_err(|_| FAILED)?;
            assert_eq!(restored, document(json!([native, archived, added])));
        }
    }
    Ok(())
}
