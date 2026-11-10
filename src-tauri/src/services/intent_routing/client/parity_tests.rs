//! Codex connection lifecycle contracts from the pinned Magpie implementation.
use super::*;

const BASE: &str = "http://127.0.0.1:1234/intent/v1";
const NATIVE: &str = "http://127.0.0.1:1234/backend-api/codex";

#[test]
fn openai_mirror_joins_native_sign_in_and_restores_its_provider(
) -> Result<(), Box<dyn std::error::Error>> {
    let (dir, manager) = tests::setup();
    let original = "model='original'\nmodel_provider='fixture-mirror'\n[model_providers.fixture-mirror]\nname='OpenAI'\nrequires_openai_auth=true\n";
    fs::write(&manager.config, original)?;
    fs::write(
        dir.path().join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"synthetic-access"}}"#,
    )?;
    let auth_before = fs::read(dir.path().join("auth.json"))?;
    for _ in 0..2 {
        manager.write_projection_with_native(
            &[tests::group()],
            "synthetic-local-token",
            BASE,
            Some(NATIVE),
        )?;
        let doc: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
        assert_eq!(doc.get("model_provider"), None);
        assert_eq!(doc["openai_base_url"].as_str(), Some(NATIVE));
        assert_eq!(
            doc["model_providers"]["fixture-mirror"]["name"].as_str(),
            Some("OpenAI")
        );
    }
    manager.restore()?;
    let doc: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
    assert_eq!(doc["model_provider"].as_str(), Some("fixture-mirror"));
    assert_eq!(doc.get("openai_base_url"), None);
    assert_eq!(fs::read(dir.path().join("auth.json"))?, auth_before);
    Ok(())
}

#[test]
fn disconnect_retains_provider_definition_without_reporting_a_connection(
) -> Result<(), Box<dyn std::error::Error>> {
    let (_dir, manager) = tests::setup();
    fs::write(&manager.config, "model='original'\n")?;
    manager.write_projection(&[tests::group()], "synthetic-local-token", BASE)?;
    let installed: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
    manager.restore()?;
    let doc: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
    assert_eq!(
        doc["model_providers"][PROVIDER],
        installed["model_providers"][PROVIDER]
    );
    assert_eq!(doc["model"].as_str(), Some("original"));
    assert!(doc.get("model_provider").is_none());
    assert!(doc.get("model_catalog_json").is_none());
    let status = manager.status()?;
    assert!(!status.enabled);
    assert!(!status.orphaned_config);
    assert_eq!(status.configuration_issue, None);
    assert!(!manager.receipt.exists());
    manager.write_projection(&[tests::group()], "synthetic-local-token", BASE)?;
    assert!(manager.status()?.configuration_owned);
    Ok(())
}

#[test]
fn provider_publication_sets_openai_actor_header_and_keeps_other_headers(
) -> Result<(), Box<dyn std::error::Error>> {
    for headers in [
        "http_headers={x-fixture='retained'}\n",
        "[model_providers.varyai.http_headers]\nx-fixture='retained'\n",
    ] {
        let (_dir, manager) = tests::setup();
        fs::write(
            &manager.config,
            format!("[model_providers.varyai]\n{headers}"),
        )?;
        manager.write_projection_inner(
            &[tests::group()],
            "synthetic-local-token",
            BASE,
            None,
            true,
            None,
        )?;
        let doc: toml::Value = fs::read_to_string(&manager.config)?.parse()?;
        let published = &doc["model_providers"][PROVIDER]["http_headers"];
        assert_eq!(published["x-fixture"].as_str(), Some("retained"));
        assert_eq!(
            published
                .get("x-openai-actor-authorization")
                .and_then(toml::Value::as_str),
            Some(PROVIDER)
        );
    }
    Ok(())
}
